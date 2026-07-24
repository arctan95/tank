use encase::ShaderType;
use glam::{IVec2, Mat2, Vec2, Vec3};

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum FontKind {
    Coptic,
    Gothic,
    MatrixCode,
    Megacity,
    Resurrections,
    HuberfishA,
    HuberfishD,
    GtargTenretniolleh,
    GtargAlientext,
    Neomatrixology,
}

#[derive(Clone, Copy)]
pub(crate) struct FontAsset {
    pub(crate) glyph_msdf_bytes: &'static [u8],
    pub(crate) glyph_msdf_label: &'static str,
    pub(crate) glint_msdf: Option<(&'static [u8], &'static str)>,
    pub(crate) glyph_sequence_length: f32,
    pub(crate) glyph_texture_grid_size: IVec2,
}

impl FontKind {
    pub(crate) fn asset(self) -> FontAsset {
        match self {
            Self::Coptic => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/coptic_msdf.png"),
                glyph_msdf_label: "Coptic MSDF",
                glint_msdf: None,
                glyph_sequence_length: 32.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Gothic => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gothic_msdf.png"),
                glyph_msdf_label: "Gothic MSDF",
                glint_msdf: None,
                glyph_sequence_length: 27.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::MatrixCode => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/matrixcode_msdf.png"),
                glyph_msdf_label: "Matrix Code MSDF",
                glint_msdf: None,
                glyph_sequence_length: 57.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Megacity => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/megacity_msdf.png"),
                glyph_msdf_label: "Megacity MSDF",
                glint_msdf: None,
                glyph_sequence_length: 64.0,
                glyph_texture_grid_size: IVec2::new(8, 8),
            },
            Self::Resurrections => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/resurrections_msdf.png"),
                glyph_msdf_label: "Resurrections MSDF",
                glint_msdf: Some((
                    include_bytes!("../matrix/assets/resurrections_glint_msdf.png"),
                    "Resurrections Glint MSDF",
                )),
                glyph_sequence_length: 135.0,
                glyph_texture_grid_size: IVec2::new(13, 12),
            },
            Self::HuberfishA => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/huberfish_a_msdf.png"),
                glyph_msdf_label: "Huberfish A MSDF",
                glint_msdf: None,
                glyph_sequence_length: 34.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::HuberfishD => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/huberfish_d_msdf.png"),
                glyph_msdf_label: "Huberfish D MSDF",
                glint_msdf: None,
                glyph_sequence_length: 34.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::GtargTenretniolleh => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gtarg_tenretniolleh_msdf.png"),
                glyph_msdf_label: "GTArg Tenretniolleh MSDF",
                glint_msdf: None,
                glyph_sequence_length: 36.0,
                glyph_texture_grid_size: IVec2::new(6, 6),
            },
            Self::GtargAlientext => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/gtarg_alientext_msdf.png"),
                glyph_msdf_label: "GTArg Alientext MSDF",
                glint_msdf: None,
                glyph_sequence_length: 38.0,
                glyph_texture_grid_size: IVec2::new(8, 5),
            },
            Self::Neomatrixology => FontAsset {
                glyph_msdf_bytes: include_bytes!("../matrix/assets/neomatrixology_msdf.png"),
                glyph_msdf_label: "Neomatrixology MSDF",
                glint_msdf: None,
                glyph_sequence_length: 12.0,
                glyph_texture_grid_size: IVec2::new(4, 4),
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TextureKind {
    Sand,
    Pixels,
    Mesh,
    Metal,
}

impl TextureKind {
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Sand => include_bytes!("../matrix/assets/sand.png"),
            Self::Pixels => include_bytes!("../matrix/assets/pixel_grid.png"),
            Self::Mesh => include_bytes!("../matrix/assets/mesh.png"),
            Self::Metal => include_bytes!("../matrix/assets/metal.png"),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Sand => "Sand Texture",
            Self::Pixels => "Pixel Grid Texture",
            Self::Mesh => "Mesh Texture",
            Self::Metal => "Metal Texture",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RippleType {
    Box,
    Circle,
}

impl RippleType {
    fn shader_value(self) -> i32 {
        match self {
            Self::Box => 0,
            Self::Circle => 1,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PaletteEntry {
    pub(crate) color: Vec3,
    pub(crate) at: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct PaletteSpec {
    entries: [PaletteEntry; 5],
    len: usize,
}

impl PaletteSpec {
    fn new(entries: &[PaletteEntry]) -> Self {
        let mut stored = [PaletteEntry {
            color: Vec3::ZERO,
            at: 0.0,
        }; 5];
        for (index, entry) in entries.iter().enumerate() {
            stored[index] = *entry;
        }
        Self {
            entries: stored,
            len: entries.len(),
        }
    }

    pub(crate) fn entries(&self) -> &[PaletteEntry] {
        &self.entries[..self.len]
    }
}

fn palette_entry(color: Vec3, at: f32) -> PaletteEntry {
    PaletteEntry { color, at }
}

fn hsl_palette_entry(hue: f32, saturation: f32, lightness: f32, at: f32) -> PaletteEntry {
    palette_entry(hsl_to_rgb(hue, saturation, lightness), at)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Vec3 {
    let a = saturation * lightness.min(1.0 - lightness);
    let f = |n: f32| {
        let k = (n + hue * 12.0) % 12.0;
        lightness - a * (-1.0_f32).max((k - 3.0).min(9.0 - k).min(1.0))
    };
    Vec3::new(f(0.0), f(8.0), f(4.0))
}

fn default_palette() -> PaletteSpec {
    PaletteSpec::new(&[
        hsl_palette_entry(0.3, 0.9, 0.0, 0.0),
        hsl_palette_entry(0.3, 0.9, 0.2, 0.2),
        hsl_palette_entry(0.3, 0.9, 0.7, 0.7),
        hsl_palette_entry(0.3, 0.9, 0.8, 0.8),
    ])
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EffectKind {
    None,
    Plain,
    Palette,
    Mirror,
}

impl EffectKind {
    pub(crate) fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) fn next_visual(self) -> Self {
        match self {
            Self::Mirror => Self::Palette,
            _ => Self::Mirror,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MatrixConfig {
    pub(crate) effect: EffectKind,
    pub(crate) font: FontKind,
    pub(crate) base_texture: Option<TextureKind>,
    pub(crate) glint_texture: Option<TextureKind>,
    pub(crate) glyph_flip: bool,
    pub(crate) glyph_rotation_degrees: f32,
    pub(crate) animation_speed: f32,
    pub(crate) glyph_sequence_length: f32,
    pub(crate) glyph_texture_grid_size: IVec2,
    pub(crate) glyph_height_to_width: f32,
    pub(crate) brightness_threshold: f32,
    pub(crate) brightness_override: f32,
    pub(crate) brightness_decay: f32,
    pub(crate) cycle_speed: f32,
    pub(crate) cycle_frame_skip: i32,
    pub(crate) fall_speed: f32,
    pub(crate) raindrop_length: f32,
    pub(crate) num_columns: u32,
    pub(crate) base_brightness: f32,
    pub(crate) base_contrast: f32,
    pub(crate) glint_brightness: f32,
    pub(crate) glint_contrast: f32,
    pub(crate) glyph_vertical_spacing: f32,
    pub(crate) glyph_edge_crop: f32,
    pub(crate) volumetric: bool,
    pub(crate) isometric: bool,
    pub(crate) density: f32,
    pub(crate) slant: f32,
    pub(crate) has_thunder: bool,
    pub(crate) ripple_type: Option<RippleType>,
    pub(crate) ripple_scale: f32,
    pub(crate) ripple_speed: f32,
    pub(crate) ripple_thickness: f32,
    pub(crate) forward_speed: f32,
    pub(crate) is_polar: bool,
    pub(crate) isolate_cursor: bool,
    pub(crate) isolate_glint: bool,
    pub(crate) loops: bool,
    pub(crate) skip_intro: bool,
    pub(crate) high_pass_threshold: f32,
    pub(crate) bloom_size: f32,
    pub(crate) bloom_strength: f32,
    pub(crate) dither_magnitude: f32,
    pub(crate) background_color: Vec3,
    pub(crate) cursor_color: Vec3,
    pub(crate) glint_color: Vec3,
    pub(crate) cursor_intensity: f32,
    pub(crate) glint_intensity: f32,
    pub(crate) palette: PaletteSpec,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        let font = FontKind::MatrixCode.asset();
        Self {
            effect: EffectKind::Palette,
            font: FontKind::MatrixCode,
            base_texture: None,
            glint_texture: None,
            glyph_flip: false,
            glyph_rotation_degrees: 0.0,
            animation_speed: 1.0,
            glyph_sequence_length: font.glyph_sequence_length,
            glyph_texture_grid_size: font.glyph_texture_grid_size,
            glyph_height_to_width: 1.0,
            brightness_threshold: 0.0,
            brightness_override: 0.0,
            brightness_decay: 1.0,
            cycle_speed: 0.03,
            cycle_frame_skip: 1,
            fall_speed: 0.3,
            raindrop_length: 0.75,
            num_columns: 80,
            base_brightness: -0.5,
            base_contrast: 1.1,
            glint_brightness: -1.5,
            glint_contrast: 2.5,
            glyph_vertical_spacing: 1.0,
            glyph_edge_crop: 0.0,
            volumetric: false,
            isometric: false,
            density: 1.0,
            slant: 0.0,
            has_thunder: false,
            ripple_type: None,
            ripple_scale: 30.0,
            ripple_speed: 0.2,
            ripple_thickness: 0.2,
            forward_speed: 0.25,
            is_polar: false,
            isolate_cursor: true,
            isolate_glint: false,
            loops: false,
            skip_intro: false,
            high_pass_threshold: 0.1,
            bloom_size: 0.4,
            bloom_strength: 0.7,
            dither_magnitude: 0.05,
            background_color: hsl_to_rgb(0.0, 0.0, 0.0),
            cursor_color: hsl_to_rgb(0.242, 1.0, 0.73),
            glint_color: hsl_to_rgb(0.0, 0.0, 1.0),
            cursor_intensity: 2.0,
            glint_intensity: 1.0,
            palette: default_palette(),
        }
    }
}

impl MatrixConfig {
    pub(crate) fn for_version(version: &str) -> Self {
        let mut config = Self::default();
        match version {
            "classic" | "2003" => {}
            "megacity" => {
                config.set_font(FontKind::Megacity);
                config.animation_speed = 0.5;
                config.num_columns = 40;
            }
            "neomatrixology" => {
                config.set_font(FontKind::Neomatrixology);
                config.animation_speed = 0.8;
                config.num_columns = 40;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.15, 0.9, 0.0, 0.0),
                    hsl_palette_entry(0.15, 0.9, 0.2, 0.2),
                    hsl_palette_entry(0.15, 0.9, 0.7, 0.7),
                    hsl_palette_entry(0.15, 0.9, 0.8, 0.8),
                ]);
                config.cursor_color = hsl_to_rgb(0.167, 1.0, 0.75);
                config.cursor_intensity = 2.0;
            }
            "operator" | "throwback" | "1999" => {
                config.cursor_color = hsl_to_rgb(0.375, 1.0, 0.66);
                config.cursor_intensity = 3.0;
                config.bloom_size = 0.6;
                config.bloom_strength = 0.75;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.01;
                config.cycle_frame_skip = 8;
                config.brightness_override = 0.22;
                config.brightness_threshold = 0.0;
                config.fall_speed = 0.6;
                config.glyph_edge_crop = 0.15;
                config.glyph_height_to_width = 1.35;
                config.ripple_type = Some(RippleType::Box);
                config.num_columns = 108;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.4, 0.8, 0.0, 0.0),
                    hsl_palette_entry(0.4, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.4, 0.8, 1.0, 1.0),
                ]);
                config.raindrop_length = 1.5;
            }
            "nightmare" => {
                config.set_font(FontKind::Gothic);
                config.isolate_cursor = false;
                config.high_pass_threshold = 0.7;
                config.base_brightness = -0.8;
                config.brightness_decay = 0.75;
                config.fall_speed = 1.2;
                config.has_thunder = true;
                config.num_columns = 60;
                config.cycle_speed = 0.35;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.0, 1.0, 0.0, 0.0),
                    hsl_palette_entry(0.0, 1.0, 0.2, 0.2),
                    hsl_palette_entry(0.0, 1.0, 0.4, 0.4),
                    hsl_palette_entry(0.1, 1.0, 0.7, 0.7),
                    hsl_palette_entry(0.2, 1.0, 1.0, 1.0),
                ]);
                config.raindrop_length = 0.5;
                config.slant = 22.5_f32.to_radians();
            }
            "paradise" => {
                config.set_font(FontKind::Coptic);
                config.isolate_cursor = false;
                config.bloom_strength = 1.0;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.005;
                config.base_brightness = -1.3;
                config.base_contrast = 2.0;
                config.brightness_decay = 0.05;
                config.fall_speed = 0.02;
                config.is_polar = true;
                config.ripple_type = Some(RippleType::Circle);
                config.ripple_speed = 0.1;
                config.num_columns = 40;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.0, 0.0, 0.0, 0.0),
                    hsl_palette_entry(0.0, 0.8, 0.3, 0.3),
                    hsl_palette_entry(0.1, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.1, 1.0, 0.6, 0.6),
                    hsl_palette_entry(0.1, 1.0, 0.9, 0.9),
                ]);
                config.raindrop_length = 0.4;
            }
            "resurrections" | "updated" | "2021" => {
                config.apply_resurrections();
            }
            "trinity" => {
                config.apply_resurrections();
                config.glint_texture = Some(TextureKind::Metal);
                config.base_texture = Some(TextureKind::Pixels);
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.131, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -0.5;
                config.glint_contrast = 1.5;
                config.base_brightness = -0.4;
                config.base_contrast = 1.5;
                config.num_columns = 60;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.37, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.37, 0.6, 0.5, 1.0),
                ]);
                config.cycle_speed = 0.01;
                config.volumetric = true;
                config.forward_speed = 0.2;
                config.raindrop_length = 0.3;
                config.density = 0.75;
            }
            "morpheus" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Mesh);
                config.base_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.333, 1.0, 0.85);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.4, 1.0, 0.5);
                config.glint_intensity = 2.0;
                config.glint_brightness = -1.5;
                config.glint_contrast = 3.0;
                config.base_brightness = -0.3;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.num_columns = 60;
                config.cycle_speed = 0.015;
                config.bloom_strength = 0.7;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.97, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.97, 0.6, 0.5, 1.0),
                ]);
                config.volumetric = true;
                config.forward_speed = 0.1;
                config.raindrop_length = 0.4;
                config.density = 0.75;
            }
            "bugs" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Sand);
                config.base_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.619, 1.0, 0.65);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.625, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -1.0;
                config.glint_contrast = 3.0;
                config.base_brightness = -0.3;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.num_columns = 60;
                config.cycle_speed = 0.01;
                config.bloom_strength = 0.7;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.12, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.14, 0.6, 0.5, 1.0),
                ]);
                config.volumetric = true;
                config.forward_speed = 0.4;
                config.raindrop_length = 0.3;
                config.density = 0.75;
            }
            "palimpsest" => {
                config.set_font(FontKind::HuberfishA);
                config.isolate_cursor = false;
                config.bloom_strength = 0.2;
                config.num_columns = 40;
                config.raindrop_length = 1.2;
                config.cycle_frame_skip = 3;
                config.fall_speed = 0.5;
                config.slant = std::f32::consts::PI * -0.0625;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.15, 0.25, 0.9, 0.0),
                    hsl_palette_entry(0.6, 0.8, 0.1, 0.4),
                ]);
            }
            "twilight" => {
                config.set_font(FontKind::HuberfishD);
                config.cursor_color = hsl_to_rgb(0.167, 1.0, 0.8);
                config.cursor_intensity = 1.5;
                config.bloom_strength = 0.1;
                config.num_columns = 50;
                config.raindrop_length = 0.9;
                config.fall_speed = 0.1;
                config.high_pass_threshold = 0.0;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.6, 1.0, 0.05, 0.0),
                    hsl_palette_entry(0.6, 0.8, 0.1, 0.1),
                    hsl_palette_entry(0.88, 0.8, 0.5, 0.5),
                    hsl_palette_entry(0.15, 1.0, 0.6, 0.8),
                ]);
            }
            "holoplay" => {
                config.set_font(FontKind::Resurrections);
                config.glint_texture = Some(TextureKind::Metal);
                config.glyph_edge_crop = 0.1;
                config.cursor_color = hsl_to_rgb(0.292, 1.0, 0.8);
                config.cursor_intensity = 2.0;
                config.isolate_glint = true;
                config.glint_color = hsl_to_rgb(0.131, 1.0, 0.6);
                config.glint_intensity = 3.0;
                config.glint_brightness = -0.5;
                config.glint_contrast = 1.5;
                config.base_brightness = -0.4;
                config.base_contrast = 1.5;
                config.high_pass_threshold = 0.0;
                config.cycle_speed = 0.01;
                config.fall_speed = 0.3;
                config.palette = PaletteSpec::new(&[
                    hsl_palette_entry(0.37, 0.6, 0.0, 0.0),
                    hsl_palette_entry(0.37, 0.6, 0.5, 1.0),
                ]);
                config.raindrop_length = 0.3;
                config.num_columns = 20;
                config.dither_magnitude = 0.0;
                config.bloom_strength = 0.0;
                config.volumetric = true;
                config.forward_speed = 0.0;
                config.density = 3.0;
            }
            "3d" => {
                config.volumetric = true;
                config.fall_speed = 0.5;
                config.cycle_speed = 0.03;
                config.base_brightness = -0.9;
                config.base_contrast = 1.5;
                config.raindrop_length = 0.3;
            }
            _ => {}
        }

        if config.bloom_size <= 0.0 {
            config.bloom_strength = 0.0;
        }

        config
    }

    fn set_font(&mut self, font: FontKind) {
        let asset = font.asset();
        self.font = font;
        self.glyph_sequence_length = asset.glyph_sequence_length;
        self.glyph_texture_grid_size = asset.glyph_texture_grid_size;
    }

    fn apply_resurrections(&mut self) {
        self.set_font(FontKind::Resurrections);
        self.glyph_edge_crop = 0.1;
        self.cursor_color = hsl_to_rgb(0.292, 1.0, 0.8);
        self.cursor_intensity = 2.0;
        self.base_brightness = -0.7;
        self.base_contrast = 1.17;
        self.high_pass_threshold = 0.0;
        self.num_columns = 70;
        self.cycle_speed = 0.03;
        self.bloom_strength = 0.7;
        self.fall_speed = 0.3;
        self.palette = PaletteSpec::new(&[
            hsl_palette_entry(0.375, 0.9, 0.0, 0.0),
            hsl_palette_entry(0.375, 1.0, 0.6, 0.92),
            hsl_palette_entry(0.375, 1.0, 1.0, 1.0),
        ]);
    }

    pub(crate) fn grid_size(self) -> [u32; 2] {
        let density = self.effective_density();
        [
            (self.num_columns as f32 * density).floor() as u32,
            self.num_columns,
        ]
    }

    fn effective_density(self) -> f32 {
        if self.volumetric && !self.effect.is_none() {
            self.density
        } else {
            1.0
        }
    }

    pub(crate) fn to_rain_uniform(self) -> RainConfigUniform {
        let grid_size = self.grid_size();
        let density = self.effective_density();
        let glyph_transform =
            Mat2::from_diagonal(Vec2::new(if self.glyph_flip { -1.0 } else { 1.0 }, 1.0))
                * Mat2::from_angle(self.glyph_rotation_degrees.to_radians());
        let slant_scale = 1.0 / ((2.0 * self.slant).sin().abs() * (2.0_f32.sqrt() - 1.0) + 1.0);
        RainConfigUniform {
            animation_speed: self.animation_speed,
            glyph_sequence_length: self.glyph_sequence_length,
            glyph_texture_grid_size: self.glyph_texture_grid_size,
            glyph_height_to_width: self.glyph_height_to_width,
            glyph_transform,
            grid_size: Vec2::new(grid_size[0] as f32, grid_size[1] as f32),
            show_debug_view: self.effect.is_none() as i32,
            brightness_threshold: self.brightness_threshold,
            brightness_override: self.brightness_override,
            brightness_decay: self.brightness_decay,
            cursor_brightness: 1.0,
            cycle_speed: self.cycle_speed,
            cycle_frame_skip: self.cycle_frame_skip,
            fall_speed: self.fall_speed,
            has_thunder: self.has_thunder as i32,
            raindrop_length: self.raindrop_length,
            ripple_scale: self.ripple_scale,
            ripple_speed: self.ripple_speed,
            ripple_thickness: self.ripple_thickness,
            ripple_type: self.ripple_type.map_or(-1, RippleType::shader_value),
            msdf_px_range: 4.0,
            forward_speed: self.forward_speed,
            base_brightness: self.base_brightness,
            base_contrast: self.base_contrast,
            glint_brightness: self.glint_brightness,
            glint_contrast: self.glint_contrast,
            has_base_texture: self.base_texture.is_some() as i32,
            has_glint_texture: self.glint_texture.is_some() as i32,
            glyph_vertical_spacing: self.glyph_vertical_spacing,
            glyph_edge_crop: self.glyph_edge_crop,
            is_polar: self.is_polar as i32,
            density,
            slant_scale,
            slant_vec: Vec2::new(self.slant.cos(), self.slant.sin()),
            volumetric: self.volumetric as i32,
            isolate_cursor: self.isolate_cursor as i32,
            isolate_glint: self.isolate_glint as i32,
            loops: self.loops as i32,
            skip_intro: self.skip_intro as i32,
            high_pass_threshold: self.high_pass_threshold,
        }
    }

    pub(crate) fn to_palette_uniform(self) -> PaletteConfigUniform {
        PaletteConfigUniform {
            dither_magnitude: self.dither_magnitude,
            background_color: self.background_color,
            cursor_color: self.cursor_color,
            glint_color: self.glint_color,
            cursor_intensity: self.cursor_intensity,
            glint_intensity: self.glint_intensity,
        }
    }
}

#[derive(Clone, Copy, ShaderType)]
pub(crate) struct RainConfigUniform {
    animation_speed: f32,
    pub(crate) glyph_sequence_length: f32,
    pub(crate) glyph_texture_grid_size: IVec2,
    glyph_height_to_width: f32,
    glyph_transform: Mat2,
    grid_size: Vec2,
    show_debug_view: i32,
    brightness_threshold: f32,
    brightness_override: f32,
    brightness_decay: f32,
    cursor_brightness: f32,
    cycle_speed: f32,
    cycle_frame_skip: i32,
    fall_speed: f32,
    has_thunder: i32,
    raindrop_length: f32,
    ripple_scale: f32,
    ripple_speed: f32,
    ripple_thickness: f32,
    ripple_type: i32,
    msdf_px_range: f32,
    forward_speed: f32,
    base_brightness: f32,
    base_contrast: f32,
    glint_brightness: f32,
    glint_contrast: f32,
    has_base_texture: i32,
    has_glint_texture: i32,
    glyph_vertical_spacing: f32,
    glyph_edge_crop: f32,
    is_polar: i32,
    density: f32,
    slant_scale: f32,
    slant_vec: Vec2,
    volumetric: i32,
    isolate_cursor: i32,
    isolate_glint: i32,
    loops: i32,
    skip_intro: i32,
    high_pass_threshold: f32,
}

#[derive(Clone, Copy, ShaderType)]
pub(crate) struct PaletteConfigUniform {
    dither_magnitude: f32,
    background_color: Vec3,
    cursor_color: Vec3,
    glint_color: Vec3,
    cursor_intensity: f32,
    glint_intensity: f32,
}
