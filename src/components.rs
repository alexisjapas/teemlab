//! Composants du *corps* d'un agent.
//!
//! [`Perception`] et [`Action`] matérialisent le contrat du cerveau :
//! *flottants normalisés en entrée → flottants en sortie*. C'est le corps qui
//! impose la forme de ces I/O.

use bevy::prelude::*;

/// Marqueur d'un agent simulé.
#[derive(Component)]
pub struct Agent;

/// Identité d'espèce / de camp. Sert de filtre de cible pour la primitive
/// d'interaction : c'est le *scénario* (via sa table de relations) qui donne un
/// sens à cet entier — relation trophique prédateur→proie, ou camp ennemi→ennemi.
/// Le moteur, lui, ne connaît que « l'espèce A peut agir sur l'espèce B ».
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Species(pub u16);

/// Réserve : la *ressource* qu'une interaction réduit, et que la prédation
/// transfère. Volontairement générique — le scénario décide si elle représente
/// de l'énergie (sélection naturelle) ou des points de vie (bataille). L'économie
/// qui l'alimente et la mort à zéro arrivent avec le scénario nº1 (item 8).
#[derive(Component, Clone, Copy, Debug)]
pub struct Reserve {
    pub current: f32,
    pub max: f32,
}

impl Reserve {
    /// Réserve pleine.
    pub fn full(max: f32) -> Self {
        Self { current: max, max }
    }

    /// Fraction de remplissage dans `[0, 1]` (0 si `max` nul).
    pub fn fraction(&self) -> f32 {
        if self.max > 0.0 {
            (self.current / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Marqueur d'un mur statique de l'arène.
#[derive(Component)]
pub struct Wall;

/// Marqueur d'une source de nourriture : une réserve d'énergie posée dans le
/// monde, mangée via la primitive d'interaction comme n'importe quelle autre
/// cible. Réensemencée pour garder l'économie soutenable.
#[derive(Component)]
pub struct Food;

/// Rayon du corps. Composant explicite pour que le code de rendu dimensionne
/// le mesh sans fouiller dans le collider Avian.
#[derive(Component, Clone, Copy, Debug)]
pub struct Radius(pub f32);

/// Magnitudes de locomotion — ce que les gènes feront varier (v1 : fixe).
#[derive(Component, Clone, Copy, Debug)]
pub struct Locomotion {
    /// Vitesse maximale.
    pub max_speed: f32,
    /// Vivacité du braquage vers la vitesse désirée, dans `[0, 1]`.
    pub agility: f32,
}

/// Capteur visuel par raycast. La *forme* — le nombre de rayons — est
/// verrouillée par espèce (v1) ; les gènes feront varier les *magnitudes*
/// (`fov`, `range`), jamais le nombre de canaux. C'est cette forme fixe qui
/// impose la taille du vecteur d'entrée du cerveau.
#[derive(Component, Clone, Copy, Debug)]
pub struct Vision {
    /// Nombre de rayons (= nombre de canaux de proximité produits).
    pub ray_count: usize,
    /// Champ de vision *total*, en radians, centré sur le cap.
    pub fov: f32,
    /// Portée d'un rayon, en unités monde.
    pub range: f32,
}

impl Vision {
    /// Décalage angulaire (radians) du rayon `i` par rapport au cap, réparti
    /// symétriquement sur `[-fov/2, +fov/2]`. Un seul rayon → droit devant.
    pub fn ray_offset(&self, i: usize) -> f32 {
        if self.ray_count <= 1 {
            0.0
        } else {
            let t = i as f32 / (self.ray_count - 1) as f32; // 0..=1
            (t - 0.5) * self.fov
        }
    }

    /// Direction monde du rayon `i` pour un agent regardant vers `facing`.
    pub fn ray_dir(&self, i: usize, facing: Vec2) -> Vec2 {
        Vec2::from_angle(facing.to_angle() + self.ray_offset(i))
    }

    /// Coût métabolique du capteur, par tick (cf. §2 « valeur, bornes, couplage
    /// de coût » et §7 « traiter la vision comme un coût »). Borne la dérive :
    /// plus de portée et de rayons = plus cher. Le *consommateur* (l'économie
    /// d'énergie) arrive avec le scénario de sélection naturelle ; ici on
    /// quantifie déjà le couplage pour qu'il n'ait qu'à soustraire.
    pub fn metabolic_cost(&self) -> f32 {
        const COST_PER_UNIT_RAY: f32 = 0.0005;
        self.range * self.ray_count as f32 * COST_PER_UNIT_RAY
    }
}

/// Instantané sensoriel. Écrit par `perceive`, lu par `decide` — conceptuellement
/// le vecteur d'entrée du cerveau. Il réunit les **canaux normalisés** (`vision`,
/// `target`, dans `[0, 1]`) et la **géométrie** qui les situe (`heading`,
/// `ray_dirs`), pour qu'un cerveau décide sans rien savoir du corps ([`Vision`]).
#[derive(Component, Default)]
pub struct Perception {
    /// Cap courant en vecteur unitaire (nul à l'arrêt).
    pub heading: Vec2,
    /// Proximité d'**obstacle** par rayon, un canal par rayon de [`Vision`], dans
    /// `[0, 1]` : `0` = rien dans la portée, `1` = au contact. Occlusion
    /// intrinsèque (chaque rayon ne retient que le hit le plus proche) ; le hit
    /// est pris quel qu'il soit — mur, agent ou nourriture.
    pub vision: Box<[f32]>,
    /// Proximité de **cible** par rayon, dans `[0, 1]` : `0` si le hit le plus
    /// proche de ce rayon n'est pas une espèce que la nôtre peut viser (table de
    /// relations, cf. [`crate::config::SimConfig::acts_on`]), sinon sa proximité.
    /// L'occlusion est incluse — une proie derrière un mur ne s'y lit pas, c'est
    /// le mur (hit le plus proche) qui occupe le rayon. Le canal que suit
    /// `Brain::Hunter`.
    pub target: Box<[f32]>,
    /// Direction **monde** (unitaire) de chaque rayon, situant les canaux
    /// ci-dessus. `perceive` la dérive déjà pour lancer le raycast ; l'exposer
    /// évite au cerveau de connaître la géométrie de [`Vision`] (fov, nombre de
    /// rayons) : un réflexe décode « rayon i → direction » sans dépendre du corps,
    /// et le contrat `Perception → Action` reste pur (un MLP ignorera ce champ).
    pub ray_dirs: Box<[Vec2]>,
}

/// Commande motrice. Écrite par `decide`, lue par `act`.
/// Conceptuellement le vecteur de sortie du cerveau.
#[derive(Component, Default)]
pub struct Action {
    /// Direction de déplacement désirée (quasi-unitaire).
    pub dir: Vec2,
    /// Fraction désirée de la vitesse max, dans `[0, 1]`.
    pub throttle: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vision(rays: usize) -> Vision {
        Vision {
            ray_count: rays,
            fov: std::f32::consts::FRAC_PI_2, // 90°
            range: 100.0,
        }
    }

    /// L'éventail est symétrique : premier et dernier rayon aux bords du FOV, et
    /// le rayon central pile sur le cap.
    #[test]
    fn ray_offsets_span_fov_symmetrically() {
        let v = vision(5);
        assert!((v.ray_offset(0) + v.fov / 2.0).abs() < 1e-6);
        assert!((v.ray_offset(4) - v.fov / 2.0).abs() < 1e-6);
        assert!(v.ray_offset(2).abs() < 1e-6);
    }

    /// Un seul rayon regarde droit devant, sans division par zéro.
    #[test]
    fn single_ray_points_forward() {
        assert_eq!(vision(1).ray_offset(0), 0.0);
    }

    /// `ray_dir` est unitaire et, cap = +X, le rayon central pointe bien en +X.
    #[test]
    fn ray_dir_is_unit_and_centered() {
        let v = vision(3);
        let d = v.ray_dir(1, Vec2::X);
        assert!((d.length() - 1.0).abs() < 1e-5);
        assert!((d - Vec2::X).length() < 1e-5);
    }

    /// Le coût croît strictement avec la portée et le nombre de rayons : c'est
    /// ce qui bornera la dérive évolutive (cf. §7).
    #[test]
    fn metabolic_cost_grows_with_range_and_rays() {
        let small = vision(3);
        let more_rays = vision(7);
        let mut longer = vision(3);
        longer.range = 200.0;
        assert!(more_rays.metabolic_cost() > small.metabolic_cost());
        assert!(longer.metabolic_cost() > small.metabolic_cost());
    }

    /// La fraction de réserve est dans `[0, 1]`, robuste à un `max` nul.
    #[test]
    fn reserve_fraction_is_clamped() {
        assert_eq!(Reserve::full(100.0).fraction(), 1.0);
        assert_eq!(
            Reserve {
                current: 50.0,
                max: 100.0
            }
            .fraction(),
            0.5
        );
        assert_eq!(
            Reserve {
                current: 0.0,
                max: 0.0
            }
            .fraction(),
            0.0
        );
        assert_eq!(
            Reserve {
                current: 999.0,
                max: 100.0
            }
            .fraction(),
            1.0
        );
    }
}
