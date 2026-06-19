//! Enregistreur vidéo **headless** (P3, item 14).
//!
//! On *re-render à frais* une run (§7 : sans déterminisme bit-à-bit, pas de
//! rejeu par seed — on relance la run et on la filme ; c'est représentatif, pas
//! le match historique exact) et on **pipe les frames brutes directement vers un
//! process `ffmpeg`** : aucun PNG intermédiaire sur disque.
//!
//! Le rendu est *réellement* sans fenêtre : on désactive `WinitPlugin`, on
//! supprime la fenêtre primaire, et la caméra rend dans une **image-cible**
//! (`RenderTarget::Image`). `ScheduleRunnerPlugin` pompe la boucle ; chaque
//! `Update` on capture l'image-cible via l'API `Screenshot` (qui fait le readback
//! GPU→CPU pour nous), et un thread dédié écrit les pixels RGBA bruts sur le
//! `stdin` de `ffmpeg`.
//!
//! Le temps avance d'un pas *fixe* par frame (`TimeUpdateStrategy::ManualDuration`,
//! = `1/fps`), indépendamment du mur d'horloge : la boucle fixe de sim joue le bon
//! nombre de ticks par frame vidéo, et la durée enregistrée est exacte.
//!
//! Run **unique**, mono-thread (la parallélisation inter-matchs est repoussée en
//! P5 avec le GA). Tout vit dans `Update` / au `Startup` — jamais de logique de
//! sim hors `FixedUpdate` (invariant cardinal) : on ne fait qu'*observer*.
//!
//! Usage : `record [scenario.ron] [--out f.mp4] [--fps N] [--seconds S]
//! [--width W] [--height H]`.

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ClearColorConfig, RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use teemlab::visuals::VisualsPlugin;
use teemlab::{SimConfig, SimPlugin};

/// Paramètres d'enregistrement, lus depuis la ligne de commande.
struct Settings {
    scenario: Option<String>,
    out: String,
    fps: f64,
    seconds: f64,
    width: u32,
    height: u32,
}

impl Settings {
    fn parse() -> Self {
        let mut s = Settings {
            scenario: None,
            out: "outputs/out.mp4".into(),
            fps: 30.0,
            seconds: 61.0,
            // Carré par défaut : l'arène est carrée, donc 1080×1080 la cadre sans
            // bandes de hors-jeu (cf. menu d'enregistrement du fenêtré).
            width: 1080,
            height: 1080,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut next = || {
                args.next().unwrap_or_else(|| {
                    eprintln!("record : valeur manquante après « {arg} »");
                    std::process::exit(2);
                })
            };
            match arg.as_str() {
                "--out" | "-o" => s.out = next(),
                "--fps" => s.fps = next().parse().expect("--fps : nombre attendu"),
                "--seconds" | "-s" => {
                    s.seconds = next().parse().expect("--seconds : nombre attendu")
                }
                "--width" | "-w" => s.width = next().parse().expect("--width : entier attendu"),
                "--height" | "-h" => s.height = next().parse().expect("--height : entier attendu"),
                other if other.starts_with('-') => {
                    eprintln!("record : option inconnue « {other} »");
                    std::process::exit(2);
                }
                // Premier argument positionnel = chemin du scénario (comme le
                // reste du projet : scénario = donnée, 1ᵉʳ argument).
                positional => {
                    if s.scenario.is_none() {
                        s.scenario = Some(positional.to_string());
                    }
                }
            }
        }
        s
    }
}

/// Handle de l'image dans laquelle la caméra rend (cible de capture).
#[derive(Resource)]
struct RecordTarget(Handle<Image>);

/// Combien de frames filmer, et leur taille.
#[derive(Resource)]
struct RecordPlan {
    width: u32,
    height: u32,
    frames: u32,
}

/// Avancement : frames demandées (screenshots lancés) vs livrées (readback reçu).
#[derive(Resource, Default)]
struct RecordProgress {
    spawned: u32,
    written: u32,
}

/// Émetteur des frames brutes vers le thread d'écriture `ffmpeg`. Le retirer du
/// `World` ferme le canal et termine proprement le thread (et donc `ffmpeg`).
#[derive(Resource)]
struct FrameSink(Sender<Vec<u8>>);

fn main() -> AppExit {
    let settings = Settings::parse();
    let config = match &settings.scenario {
        Some(path) => SimConfig::from_ron_file(path).unwrap_or_else(|err| {
            eprintln!("record : scénario « {path} » illisible : {err}");
            std::process::exit(1);
        }),
        None => SimConfig::default(),
    };
    let frames = (settings.fps * settings.seconds).round().max(1.0) as u32;

    // On crée le dossier de sortie au besoin (par défaut `outputs/`, ignoré par
    // git) — ffmpeg n'écrit pas dans une arborescence manquante.
    if let Some(parent) = std::path::Path::new(&settings.out).parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "record : dossier de sortie « {} » impossible : {err}",
            parent.display()
        );
        std::process::exit(1);
    }

    // `ffmpeg` lit du rawvideo RGBA sur stdin → encode en H.264/yuv420p. Pas de
    // fichier intermédiaire : on branche directement le pipe.
    let mut child: Child = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", settings.width, settings.height),
            "-framerate",
            &format!("{}", settings.fps),
            "-i",
            "-",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            &settings.out,
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!("record : impossible de lancer ffmpeg ({err}). Est-il installé ?");
            std::process::exit(1);
        });

    let stdin = child.stdin.take().expect("stdin ffmpeg piped");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    // Thread d'écriture : tout le pipe vers ffmpeg vit hors de la boucle Bevy,
    // pour ne pas bloquer le rendu sur les I/O. Il tourne tant qu'un émetteur
    // existe ; sa fin ferme le stdin de ffmpeg → finalisation du fichier.
    let writer = std::thread::spawn(move || feed_ffmpeg(stdin, rx));

    let frame_dt = Duration::from_secs_f64(1.0 / settings.fps);
    let mut app = App::new();
    app.add_plugins(
        // Rendu réel mais sans fenêtre : pas de winit (c'est ScheduleRunnerPlugin
        // qui pilote la boucle), pas de fenêtre primaire — la caméra rend dans une
        // image, pas dans une surface.
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
    .add_plugins(SimPlugin::new(config))
    .add_plugins(VisualsPlugin)
    // Temps piloté : chaque update avance d'exactement 1/fps, donc la boucle fixe
    // joue le bon nombre de ticks et la vidéo est cadencée au mur près.
    .insert_resource(TimeUpdateStrategy::ManualDuration(frame_dt))
    .insert_resource(RecordPlan {
        width: settings.width,
        height: settings.height,
        frames,
    })
    .insert_resource(FrameSink(tx))
    .init_resource::<RecordProgress>()
    .add_systems(Startup, setup_recorder)
    .add_systems(Update, capture_frame);

    eprintln!(
        "record : {} frames à {} fps ({:.1}s), {}×{} → {}",
        frames, settings.fps, settings.seconds, settings.width, settings.height, settings.out
    );
    let exit = app.run();

    // Fin de run : on lâche l'émetteur restant (la ressource) pour fermer le
    // canal, on attend la fin de l'écriture, puis la finalisation de ffmpeg.
    app.world_mut().remove_resource::<FrameSink>();
    let _ = writer.join();
    match child.wait() {
        Ok(status) if status.success() => eprintln!("record : vidéo écrite."),
        Ok(status) => eprintln!("record : ffmpeg a terminé avec {status}."),
        Err(err) => eprintln!("record : attente de ffmpeg échouée : {err}"),
    }
    exit
}

/// `Startup` : crée l'image-cible et la caméra qui rend dedans, cadrée sur l'arène.
fn setup_recorder(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    plan: Res<RecordPlan>,
    config: Res<SimConfig>,
) {
    let size = Extent3d {
        width: plan.width,
        height: plan.height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Cible de rendu *et* source de copie (pour le readback du screenshot).
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::TEXTURE_BINDING;
    let handle = images.add(image);

    // Cadrage : l'arène (± half_extent) tient toujours, avec une marge.
    let span = config.arena_half_extent * 2.0 * 1.1;
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.05)),
            ..default()
        },
        // En 0.18 la cible de rendu est un composant à part, requis par `Camera`.
        RenderTarget::from(handle.clone()),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: span,
                min_height: span,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));

    commands.insert_resource(RecordTarget(handle));
}

/// `Update` : tant qu'il reste des frames à filmer, demande une capture de
/// l'image-cible. L'observateur (déclenché quand le readback GPU→CPU est prêt)
/// pousse les pixels vers le thread ffmpeg et, à la dernière frame livrée, sort.
fn capture_frame(
    mut commands: Commands,
    target: Res<RecordTarget>,
    sink: Res<FrameSink>,
    plan: Res<RecordPlan>,
    mut progress: ResMut<RecordProgress>,
) {
    if progress.spawned >= plan.frames {
        return;
    }
    progress.spawned += 1;
    let tx = sink.0.clone();
    // Une capture par frame rendue : le pipeline de rendu les livre dans l'ordre
    // de soumission et le canal est FIFO → l'ordre des frames est préservé.
    commands.spawn(Screenshot::image(target.0.clone())).observe(
        move |captured: On<ScreenshotCaptured>,
              plan: Res<RecordPlan>,
              mut progress: ResMut<RecordProgress>,
              mut exit: MessageWriter<AppExit>| {
            if let Some(data) = captured.image.data.clone() {
                // Canal plein/fermé = thread ffmpeg parti : rien à faire de
                // plus, la fin de run gérera la sortie.
                let _ = tx.send(data);
            }
            progress.written += 1;
            if progress.written >= plan.frames {
                exit.write(AppExit::Success);
            }
        },
    );
}

/// Thread d'écriture : draine les frames brutes et les pousse sur le stdin de
/// ffmpeg. S'arrête quand tous les émetteurs sont lâchés (fin de run), puis ferme
/// le stdin (via `drop`) pour que ffmpeg finalise le fichier.
fn feed_ffmpeg(mut stdin: std::process::ChildStdin, rx: Receiver<Vec<u8>>) {
    while let Ok(frame) = rx.recv() {
        if stdin.write_all(&frame).is_err() {
            // ffmpeg a fermé son entrée (erreur d'encodage) : inutile d'insister.
            break;
        }
    }
    let _ = stdin.flush();
    // `stdin` est droppé ici → EOF côté ffmpeg → finalisation.
}
