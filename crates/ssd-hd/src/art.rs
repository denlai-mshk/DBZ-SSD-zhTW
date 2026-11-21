//! The approved art in the Enhanced picture: portraits and faces found through the pack's
//! tile binding, and battle poses and effects found by the key of their pixels (the pack's
//! `art.bin` is [`crate::art_index`]).
//!
//! While the renderer draws the 1x frame it records, for every pixel, what the art passes need
//! ([`ArtPixel`]): the picture and cell a bound character belongs to, the OAM entry an object
//! pixel comes from, and the colour math of the pixel. The pictures are drawn over the picture
//! the look made; a pose replaces the sprites it was made from: the frame is drawn a second time
//! with those sprites left out (the picture under them), and the pose's art is drawn over that,
//! where the sprites were visible in the first one. Nothing here reads anything but the render log
//! and the pack, so the picture is a function of the frame and the pack.
//!
//! The game draws colour variants of an enemy, and the same face in several palette rows, from the
//! same tiles. Art is made for one palette: where an entry matches by its tiles or shape but not
//! by its palette, the art is drawn recoloured to the palette the game uses ([`recolour`]); an
//! entry made for the exact palette is always preferred.

mod bg_cache;
mod bgshadow;
mod draw;
mod field;
mod load;
#[cfg(test)]
mod load_tests;
mod partial;
mod portrait_check;
pub(crate) mod poses;
mod reader;
mod recolour;
mod resample;
#[cfg(test)]
pub(crate) mod tests;

pub use portrait_check::PortraitUse;
pub use tally::{Fate, FrameTally, ItemKey, TallyCount, TallyItem, TallyLayer};
mod tally;

pub use load::art_loads_on_draw;
pub use resample::Image;

/// `image`, made for the palette `source`, in the colours of `row` (see [`recolour::recolour`]).
pub(crate) fn recolour_to(
    image: &Image,
    source: &crate::art_index::Palette,
    row: &[[u8; 3]; 16],
) -> Image {
    recolour::recolour(image, source, row)
}

use crate::art_index::Palette;
use crate::bgart::{self, BgArt, BgImage, Probe, Slot};
use crate::binding::Binder;
use crate::pack::{Pack, PoseVariant};
use crate::screen::{Layer, Plan};
use crate::transfer::{Colouring, Colours, Transfer};
use poses::{Pose, ROW_OF_CAPTURE};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use ssd_oracle::{FrameLog, LineRegs};
use std::collections::VecDeque;
use std::sync::Arc;

/// `ArtPixel::picture` of a pixel with no picture art.
pub(crate) const NO_PICTURE: u32 = u32::MAX;

/// `ArtPixel::asset` of a pixel whose character is bound to no asset of the pack.
pub(crate) const NO_ASSET: u32 = u32::MAX;

/// What layer a recorded pixel comes from (the tally of a tour, see [`FrameTally`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// Nothing is drawn there (or the tally is off).
    Nothing,
    Bg,
    Obj,
    /// A Mode 7 plane pixel.
    Plane,
}

/// The art index of a pose that has no art: a ground shadow, drawn from the frame's own pixels.
pub(crate) const SHADOW: u32 = u32::MAX;

/// The bit of an object index (in the poses found) that says the art is drawn mirrored left to
/// right: the pose is the art's pose facing the other way.
pub(crate) const MIRROR: u32 = 1 << 30;

/// The index of the object `at` names, without the mirror bit (never [`SHADOW`]).
pub(crate) fn real(at: u32) -> usize {
    (at & !MIRROR) as usize
}

/// Where the art of a pose came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArtSource {
    /// The index has no art for it (or it is a ground shadow, drawn from the frame's pixels).
    #[default]
    None,
    /// The art made for the pose's own pixels.
    Exact,
    /// The art made for the pose facing the other way, drawn mirrored.
    Mirror,
    /// The art of a pose of the same shape in other colours (a palette variant).
    Shaped,
    /// A piece of the art of a bigger pose of the index (a fade-in's stripes, a cut sprite).
    Held,
}

/// The fewest opaque pixels of a pose found by its shape.
const SHAPE_MIN_OPAQUE: usize = 150;

/// The share (per mille) of a pose's opaque pixels with no neighbour left or right from which a pose
/// is a stripy fade-in, which the capture left out and may be part of a pose of the index.
const STRIPY_PARTIAL: u32 = 200;

/// The least share (per cent) of the opaque pixels of the pose of the index that a sprite cut by the
/// screen's edge shows, for it to be taken for a piece of that pose.
const CUT_MIN_SHARE: u32 = 30;

/// Partial poses whose holding pose is remembered by their key.
const PARTIAL_CACHE: usize = 16384;

/// What a recoloured art is kept by: a picture or an object, its index, the key of the pose it was
/// read for (0 when its own palette says what it was made for), and the colours it was drawn in.
type VariantKey = (bool, u32, u64, [[u8; 3]; 16]);

/// The bytes of recoloured art kept (a palette fade makes a new variant every frame).
const VARIANT_BYTES: usize = 96 << 20;

/// Objects whose art is kept decoded (the least recently used one goes first).
const OBJECT_CACHE: usize = 256;

/// Pictures of a layer's art whose colours are numbered and kept (see [`ArtState::numbered_colours`]).
const NUMBERED_PICTURES: usize = 16;

/// The picture of the pose variant an object is drawn from (see [`ArtState::pose_variant`]): with its margin, asked of
/// the reading thread and not read yet, or none.
enum VariantArt {
    Drawn(Arc<Image>, i32),
    Asked,
    None,
}

/// `image` flipped left to right: the art of a pose facing the other way, every row read right to left.
fn mirror(image: &Image) -> Image {
    let mut rgba = Vec::with_capacity(image.rgba.len());
    for row in image.rgba.chunks(image.width * 4) {
        for pixel in row.chunks(4).rev() {
            rgba.extend_from_slice(pixel);
        }
    }
    let mut mirrored = Image::new(image.width, image.height, rgba);
    mirrored.luma_ref = image.luma_ref;
    mirrored
}

/// What a four-bit character shows of the pictures.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Choice {
    /// No picture covers it.
    None,
    /// A character of a portrait asset that has no picture: counted, nothing drawn.
    Portrait,
    /// Cell `1` of picture `0`.
    Picture(u32, u8),
}

/// What the art passes need of one pixel of the 1x frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtPixel {
    /// The picture of the art index the pixel's character is part of, or [`NO_PICTURE`].
    pub picture: u32,
    /// The character's cell in the 4 x 4 picture, and the pixel's place in the character
    /// ([`crate::pos`]).
    pub cell: u8,
    pub pos: u8,
    /// The OAM entry of an object pixel ([`crate::NO_OBJECT`]).
    pub oam: u8,
    /// The CGRAM row (16 colours each, the objects' from 8) of a pixel with a picture.
    pub row: u8,
    /// The priority number the pixel won with (lower is in front), and whether it is in mode 0 or 1
    /// (which says how an object's priority bits rank).
    pub prio: u8,
    pub mode_01: bool,
    pub plan: Plan,
    pub brightness: u8,
    /// What the pixel shows, filled only while the tally is on ([`ArtState::set_tally`]): its
    /// layer, the bit depth and VRAM word address of its character, and the pack tile the
    /// character is bound to ([`NO_ASSET`] when none).
    pub shown: Shown,
    pub bpp: u8,
    pub char: u16,
    pub asset: u32,
    pub tile: u32,
    /// What the art of the backgrounds needs of the pixel (see [`bgart`]).
    pub bg: bgart::BgRecord,
    /// For a pixel of a four-bit BG character: the character's VRAM word address, the pixel's
    /// place in it ([`crate::pos`]) and the palette number the BG gives it (a fighter drawn as BG
    /// tiles is read from these); `bg4` says they are set.
    pub bg4: bool,
    pub bg_char: u16,
    pub bg_pos: u8,
    pub bg_palette: u8,
}

impl ArtPixel {
    /// Whether an object with OAM priority bits `bits` would be in front of this pixel.
    pub(crate) fn beaten_by(&self, bits: u8) -> bool {
        object_priority(self.mode_01, bits) <= self.prio
    }

    pub(crate) const NONE: Self = Self {
        picture: NO_PICTURE,
        cell: 0,
        pos: 0,
        oam: crate::NO_OBJECT,
        row: 0,
        prio: u8::MAX,
        mode_01: true,
        plan: Plan { force_black: false, math: None },
        brightness: 15,
        shown: Shown::Nothing,
        bpp: 0,
        char: 0,
        asset: NO_ASSET,
        tile: 0,
        bg: bgart::BgRecord::NONE,
        bg4: false,
        bg_char: 0,
        bg_pos: 0,
        bg_palette: 0,
    };
}

/// What one pose of a frame came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseStat {
    /// A fighter (palette 1 or 2) or an effect.
    pub fighter: bool,
    /// A sprite group of the field (neither a fighter nor an effect).
    pub field: bool,
    /// A ground shadow (one dark flat ellipse), which the renderer draws from the game's own shape.
    pub shadow: bool,
    /// The OAM entries the pose draws or hides (bit n: entry n), the ones another pose took not counted.
    pub sprites: u128,
    /// The OAM entries the pose had but another pose took (the ones `sprites` leaves out).
    pub excluded: u128,
    /// The pose's sprites mostly show a picture's pixels (a portrait drawn as sprites): it is that
    /// picture, not a pose of its own (see `is_picture`).
    pub picture: bool,
    /// The art index has art for the pose, and it was drawn.
    pub matched: bool,
    /// The art was made for another palette than the pose's, and was drawn recoloured.
    pub recoloured: bool,
    /// Pixels of the frame the pose's sprites show.
    pub visible: u32,
    /// Opaque pixels of the pose, and the share of them with no opaque neighbour left or right
    /// (per mille): the capture left poses of fewer than 40 pixels or 200 per mille out.
    pub opaque: u32,
    pub stripy: u32,
    pub key: u64,
    /// The OAM palette of the pose's first sprite.
    pub palette: u8,
    /// Where the pose's art came from, and which object of the index it is (when it has art).
    pub source: ArtSource,
    pub art: Option<u32>,
    /// Where the pose's array sits (x of the screen, row of the capture) and its size.
    pub org: (i32, i32),
    pub size: (usize, usize),
    /// What the original shows inside the pose's box, as a digest that does not depend on how the
    /// sprites are ordered or grouped (see [`poses::region_digest`]); filled while the art measures.
    pub look: u64,
}

/// A pose of a frame that has no art (a missing or a partial one) with its pixels, for the tour to
/// write out: what art is made from. Kept only while the tally is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseShape {
    pub key: u64,
    pub fighter: bool,
    /// A sprite group of the field (neither a fighter nor an effect).
    pub field: bool,
    /// A ground shadow (one dark flat ellipse), which the renderer draws from the game's own shape.
    pub shadow: bool,
    /// What became of the pose (never [`Fate::Art`]).
    pub fate: Fate,
    pub w: usize,
    pub h: usize,
    /// The trimmed palette-index array, row by row (0 transparent).
    pub arr: Vec<u8>,
    /// An effect's trimmed colours (RGBA, row by row); empty for a fighter.
    pub rgba: Vec<u8>,
    /// Where the array's top-left pixel is on the screen (x, and the capture's row).
    pub org: (i32, i32),
    /// The OAM palette of the pose's first sprite and the colours of that palette's row.
    pub palette: u8,
    pub row: [[u8; 3]; 16],
}

/// The kinds of pose the pixels of the sprites are counted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fighter = 0,
    Effect = 1,
    Field = 2,
}

impl Kind {
    fn of(pose: &Pose) -> Self {
        if pose.field {
            Self::Field
        } else if pose.fighter {
            Self::Fighter
        } else {
            Self::Effect
        }
    }
}

/// What a pose drawn from art drew in a frame: the colours it drew against those of the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnPose {
    pub key: u64,
    pub fighter: bool,
    /// A sprite group of the field.
    pub field: bool,
    /// The art is a luma shape.
    pub luma: bool,
    pub drawn: Drawn,
}

/// Pixels of the frame that the sprites of fighters (or of effects) show, by what became of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KindPixels {
    /// All of them.
    pub total: u64,
    /// Of a pose whose art was drawn.
    pub art: u64,
    /// Of those, the pixels of poses whose art was made for another palette (drawn recoloured).
    pub recoloured: u64,
    /// Of a pose the capture would have kept (40 pixels, not stripy) with no art in the index.
    pub missing: u64,
    /// Of a pose the capture left out (fewer than 40 pixels, or stripy: a fade-in, a cut sprite).
    pub partial: u64,
}

/// What a picture cost to draw, by its stages, for the log of a slow window frame: times from `Instant`s and counts
/// only, so it is kept for every picture. The art's own stages are zero for a picture drawn without art.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrawCost {
    /// The whole picture (`WideRenderer::render_hd`, or the Enhanced renderer's as its caller times it).
    pub total: std::time::Duration,
    /// Finding the background layers and the Mode 7 plane the frame shows (`ArtState::begin_bg`).
    pub begin_bg: std::time::Duration,
    /// Finding the poses in the frame's sprites.
    pub find_poses: std::time::Duration,
    /// The renderer's own passes over the frame (the wide view's compose, every pass of it).
    pub draw: std::time::Duration,
    /// Drawing the art over the picture (`ArtState::compose`).
    pub art_compose: std::time::Duration,
    /// Reading and scaling the art files the caches lacked on the threads that draw, summed over the threads that read
    /// them: inside the stages above, not beside them. The art read by its own thread ([`crate::Renderer::set_art_async`])
    /// is not drawing's time and is not counted.
    pub loads: std::time::Duration,
    /// The art files the caches lacked and the picture asked for: read there and then, or asked of the reading thread
    /// (once a picture for each file).
    pub requests: u32,
    /// The art files received into the caches, by kind: background layers, pictures, objects, pose variants, Mode 7
    /// planes. Read there and then, a file is asked for and received at once; read by its own thread, it is received
    /// when a picture starts after it is read (see [`DrawCost::received`]).
    pub bg_loads: u32,
    pub pictures: u32,
    pub objects: u32,
    pub variants: u32,
    pub planes: u32,
    /// The bytes of the files received (counted when they are received).
    pub load_bytes: u64,
    /// The background layers the frame is bound to, and whether one is the Mode 7 plane.
    pub bg_slots: u32,
    pub plane: bool,
    /// The poses found in the frame's sprites.
    pub poses: u32,
    /// The background layers the cache let go to keep within its budget, the one used longest ago first
    /// (`bg_cache::BgImages::insert`).
    pub bg_evicted: u32,
}

impl DrawCost {
    /// Adds `other`, a picture drawn after this one, to it: times and counts add, the bindings are the last one's.
    pub fn add(&mut self, other: &DrawCost) {
        self.total += other.total;
        self.begin_bg += other.begin_bg;
        self.find_poses += other.find_poses;
        self.draw += other.draw;
        self.art_compose += other.art_compose;
        self.loads += other.loads;
        self.requests += other.requests;
        self.bg_loads += other.bg_loads;
        self.pictures += other.pictures;
        self.objects += other.objects;
        self.variants += other.variants;
        self.planes += other.planes;
        self.load_bytes += other.load_bytes;
        self.bg_slots = other.bg_slots;
        self.plane = other.plane;
        self.poses = other.poses;
        self.bg_evicted += other.bg_evicted;
    }

    /// The art files received, of every kind.
    #[must_use]
    pub fn received(&self) -> u32 {
        self.bg_loads + self.pictures + self.objects + self.variants + self.planes
    }
}

/// The kinds of art file a frame reads (see [`DrawCost`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum LoadKind {
    Bg,
    Picture,
    Object,
    Variant,
    Plane,
}

/// The art files a frame asked for and received, and what reading them on the threads that draw cost: counted where
/// they are asked for (read there and then, or asked of the reading thread) and where they are received into the
/// caches, on any thread of the pool.
#[derive(Debug, Default)]
pub(crate) struct LoadCounters {
    requests: std::sync::atomic::AtomicU32,
    received: [std::sync::atomic::AtomicU32; 5],
    /// The bytes of the files received: added by the reading of a file the frame reads itself, or when the reading
    /// thread's file is received.
    pub(crate) bytes: std::sync::atomic::AtomicU64,
    nanos: std::sync::atomic::AtomicU64,
}

impl LoadCounters {
    /// Counts a file of `kind` read by the frame from now, asked for and received at once (its bytes are counted as
    /// they are read, see [`LoadCounters::bytes`]); the time is counted when the guard returned goes.
    fn start(&self, kind: LoadKind) -> LoadTimer<'_> {
        self.request();
        self.received[kind as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        LoadTimer { counters: self, started: std::time::Instant::now() }
    }

    /// Counts a file asked for.
    pub(crate) fn request(&self) {
        self.requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Counts a file of `kind` and `bytes` the reading thread read, received into the caches (its time is not the
    /// frame's).
    pub(crate) fn receive(&self, kind: LoadKind, bytes: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.received[kind as usize].fetch_add(1, Relaxed);
        self.bytes.fetch_add(bytes, Relaxed);
    }

    /// Puts the counts into `cost`.
    fn fill_cost(&self, cost: &mut DrawCost) {
        use std::sync::atomic::Ordering::Relaxed;
        let count = |kind: LoadKind| self.received[kind as usize].load(Relaxed);
        cost.requests = self.requests.load(Relaxed);
        cost.bg_loads = count(LoadKind::Bg);
        cost.pictures = count(LoadKind::Picture);
        cost.objects = count(LoadKind::Object);
        cost.variants = count(LoadKind::Variant);
        cost.planes = count(LoadKind::Plane);
        cost.load_bytes = self.bytes.load(Relaxed);
        cost.loads = std::time::Duration::from_nanos(self.nanos.load(Relaxed));
    }
}

/// The time of one file read, counted when it goes (see [`LoadCounters::start`]).
struct LoadTimer<'a> {
    counters: &'a LoadCounters,
    started: std::time::Instant,
}

impl Drop for LoadTimer<'_> {
    fn drop(&mut self) {
        let nanos = u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.counters.nanos.fetch_add(nanos, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What the art did in the last frame rendered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtStats {
    pub fighters: KindPixels,
    pub effects: KindPixels,
    /// The sprites of the field's groups (see [`poses::field_groups`]); with the fighters and the
    /// effects it counts the same sprite again where it is in both kinds of group.
    pub field: KindPixels,
    /// Every pixel the object layer shows, inside the original window and in the margins of the
    /// wide view (where the field's objects the original drops are drawn), by what became of the
    /// sprite that shows it: the art covers the margins as it covers the window, so the two
    /// shares of `art` match.
    pub sprites_window: KindPixels,
    pub sprites_margin: KindPixels,
    /// Pixels of the groups that the edge of the view cuts (or that run into the top or the bottom
    /// of the picture, where the capture's sprites end) that no art holds, and of the sprites that
    /// make no group (fewer than 40 pixels): the pieces that remain are too few to find their
    /// object by, in the window as in the margins, and are left out of the two above.
    pub sprites_cut: u64,
    pub poses: Vec<PoseStat>,
    /// What each pose drawn from art drew, with its colours, when the state measures them.
    pub drawn: Vec<DrawnPose>,
    /// Pixels drawn from a picture (portraits and faces), and pixels of characters of the pack's
    /// portrait assets that had no picture.
    pub picture_pixels: u64,
    /// Of those, the pixels drawn in the palette row the game uses where the picture was made for
    /// another palette (recoloured).
    pub picture_recoloured: u64,
    pub portrait_pixels_without_art: u64,
    /// Pixels of battle backgrounds (the layers of [`bgart::BATTLE_LAYERS`] and their backdrop,
    /// where nothing is in front), and the ones drawn from the art of a layer.
    pub bg_pixels: u64,
    pub bg_art: u64,
    /// The frame was drawn again without BG1 for the picture under the plane (a plane of art with `replace`): 1, else 0.
    pub plane_under: u64,
    /// The map art of the field frame: the layer of the maps' index its map data names
    /// (`available`, for a frame whose BG2 shows the field's map in whatever main loop state) and
    /// whether the frame draws it (`bound`: the frame is classed as the field and its layer slot was
    /// bound to that art).
    pub field_map: FieldMapArt,
    /// The entries of the backgrounds' index whose content the frame shows and that it does not draw
    /// (the layer's slot was lost: VRAM loaded while it is displayed, or the field's map took its
    /// place).
    pub bg_unbound: Vec<u32>,
    /// The art files (layers, pictures, objects, pose variants, planes) the frame read itself: none
    /// when the art is read by its own thread (see [`crate::art_loads_on_draw`]).
    pub loads_on_draw: u32,
}

/// See [`ArtStats::field_map`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldMapArt {
    pub available: Option<u32>,
    pub bound: Option<u32>,
    /// The tiles of the art the frame read as it was drawn: no read ahead had brought them.
    pub read: u32,
    /// The tiles of the art the reading thread brought since the frame before (read ahead).
    pub read_ahead: u32,
}

/// The Mode 7 plane a frame shows: the matrix of each line (by scanline; `None` for a line that
/// does not show the plane) and the words of VRAM the plane is made of.
pub(crate) struct PlaneFrame {
    lines: Vec<Option<crate::mode7::Plane>>,
    vram: Vec<u16>,
}

/// The work RAM bytes the conditions of a pack's pose variants are read from (`docs/re/data/kaioken-bp.md`, section 4).
const KAIOKEN_STATE: usize = 0x0C36;
/// The Kaioken's multiplier in use (`$01EF`, 0 = none; `docs/re/data/kaioken-bp.md`, section 4): written when the move is
/// set up, before `$0C36` says the Kaioken is on, so the close-up of its first use ("悟空的 N倍界王拳！") has it. It is not
/// cleared when the Kaioken ends: only the start of the next turn clears it (`05:83D8`, the one `STZ $01EF` of the ROM,
/// with `$0C36` clear), so a multiplier alone is not the Kaioken on (see [`Conditions::of_work_ram`]).
const KAIOKEN_MULTIPLIER: usize = 0x01EF;
/// `battle_phase` (`$AB`) and the phase of Goku's Super Saiyan transformation (`$1A`): the trigger (`05:8772..87B3`, the
/// one place of the ROM that writes `$1A` there, `05:87B1`) clears `$0C36`, `$0C37` and `$01EE` but not
/// [`KAIOKEN_MULTIPLIER`]. (The app's watch of the transformation, `ssd-app`'s `voice.rs`, reads the same byte and value
/// with constants of its own: that crate is not changed for this one.)
const BATTLE_PHASE: usize = 0x00AB;
const SUPER_SAIYAN_PHASE: u8 = 0x1A;
/// The party records: nine of them from `$0B00`, `$20` bytes each, the unit in the second byte.
const PARTY_RECORDS: usize = 0x0B00;
const RECORD_STRIDE: usize = 0x20;
const PARTY_SLOTS: usize = 9;
/// The unit of Super Saiyan Goku.
const SUPER_SAIYAN_GOKU: u8 = 0x3B;
/// `game_mode` (`$24`, bit 7 aside): what the main loop runs, `$11` the ending (`docs/re/data/scenes.md`).
const GAME_MODE: usize = 0x0024;
const ENDING_MODE: u8 = 0x11;
/// `$73` (a word): the frames of the ending's loop (`00:B07C`), counted by `00:B0E8`. `00:B0EE` draws Goku from the sprites
/// of the flight with Gohan (`00:BCA7`) while it is under [`ENDING_FLIGHT_END`], and from those of him crouching with
/// Gohan before Vegeta (`00:BC55`) from then on when Vegeta is in the ending's party ([`ENDING_PARTY`] bit 7); without
/// Vegeta he flies on, out of the screen.
const ENDING_FRAMES: usize = 0x0073;
/// The frame of the ending's loop from which Goku crouches with Gohan before Vegeta (`00:B0F0`, `CPX #$1F40`).
const ENDING_FLIGHT_END: u16 = 0x1F40;
/// `$82`: who is in the ending (`00:96C5`, a bit for each member of the party; bit 0 Goku, bit 1 Gohan, bit 7 Vegeta).
const ENDING_PARTY: usize = 0x0082;
const ENDING_VEGETA: u8 = 0x80;
/// The mark of a unit of Goku's art in the index (`battle-goku-NNN`, `gallery-party-goku-u00-NNN`, see [`ArtState::unit_of`]).
pub(crate) const GOKU_MARK: &str = "goku";
/// The mark of a unit of Super Saiyan Goku's art (`battle-goku-ssj-NNN`, `gallery-party-goku-ssj-NNN`).
pub(crate) const SUPER_SAIYAN_GOKU_MARK: &str = "goku-ssj";
/// The mark of the art of Super Vegeta's battle poses in the index (`battle-super-vegeta-NNN`): its palettes are the
/// colours the game draws him in (see [`ArtState::recolourable`]).
pub(crate) const SUPER_VEGETA_MARK: &str = "battle-super-vegeta";
/// The indexes of Super Vegeta's hair in his palette (yellow and orange; black in Vegeta's, the colour of the outline):
/// what tells his colours from Vegeta's where a pose's art does not use them.
pub(crate) const SUPER_VEGETA_HAIR: u16 = 1 << 14 | 1 << 15;
/// Super Vegeta's palette in GAME OVER (BGR555, index 0 left out): the scene draws him flying over the words with the
/// tiles of the effect's art `gallery-fx-1118`, in a row of his colours in another order (white 2, grey 3 and 14, skin
/// 11 to 13) and with another yellow and orange for his hair (4 and 15). The ROM's data at `$12:FE82` (`LoROM`, file
/// offset `0x97E82`), the same in the frames of four replays that end in it; the row before it (`$12:FE62`) is the same
/// with black at 4 and 15, Vegeta's hair. See [`game_over_super_vegeta`].
const GAME_OVER_SUPER_VEGETA: [u16; 16] = [
    0x0000, 0x0000, 0x7FFF, 0x3DEF, 0x03BF, 0x2ABC, 0x1A38, 0x09B4, 0x71EB, 0x5107, 0x3862, 0x42FF,
    0x2639, 0x0952, 0x56B5, 0x031F,
];

/// [`GAME_OVER_SUPER_VEGETA`] as the palette of a pose (every index but 0): the palette of the picture of
/// `art-override\super-vegeta\` for an effect's pose (patch 0049, `Pack::super_vegeta_override`), drawn only where the
/// sprite's palette is this one (see [`ArtState::recolourable`]).
pub(crate) fn game_over_super_vegeta() -> Palette {
    let mut colors = [[0_u8; 3]; 16];
    for (colour, &word) in colors.iter_mut().zip(&GAME_OVER_SUPER_VEGETA) {
        *colour = poses::rgb555(word);
    }
    Palette { mask: 0xFFFE, colors }
}
/// The indexes of Goku's hair in his palettes: black in the normal Goku's (red with the Kaioken), yellow and gold in the
/// Super Saiyan's.
pub(crate) const GOKU_HAIR: u16 = 1 << 14 | 1 << 15;
/// The key of Goku crouching with Gohan in his arms before Super Vegeta, the ending's scene after the flight
/// (`battle-goku-017` and `battle-goku-ssj-021`). The game draws him there with black hair, in the normal palette;
/// this build draws that pose from the Super Saiyan art (with the pack's variant of the hair standing up) while unit
/// `$3B` is in the party, whatever the colours, because the fight with Super Vegeta comes next. A choice this
/// branch makes about the story, not what the original game shows (see [`ArtState::unit_for`]).
const GOKU_HOLDING_GOHAN: u64 = 0x3326_F5A6_5ECC_E1EB;
/// The key of the pixels of `battle-vegeta-001` (34 x 42, Vegeta standing), which the game also draws, in Super Vegeta's
/// colours and with OAM palette 0, as Super Vegeta flying through space after the party loses the hidden fight with him.
/// There the sprites are a field group with the battle pose's key, and a field group finds only the field's art, so his
/// picture of `art-override\super-vegeta\` (patch 0049) was never found and the game's sprite (with the pads) was drawn.
/// This field group is drawn from that picture when it can be drawn in the frame's colours (see
/// [`ArtState::ending_space_vegeta`]).
const ENDING_SPACE_VEGETA: u64 = 0xAA5F_9D99_2371_E593;

/// The conditions a pack's pose variants name (`[[pose_variant]]`, `ssd_mod::pose::Condition`), as the game's work RAM of a
/// frame shows them. The Enhanced picture draws a variant only while its condition holds. `super_saiyan_goku` is the party
/// record of Super Saiyan Goku (see [`ArtState::unit_for`]); `ending_flight` the ending's flight with Gohan on screen
/// (see [`ArtState::ending_goku`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Conditions {
    kaioken: bool,
    super_saiyan_goku: bool,
    /// The Kaioken is on (`$0C36` bit 7) or its multiplier is in use (`$01EF`), whatever the party: the normal Goku using
    /// it (see [`ArtState::recolourable`], patch 0050). The multiplier is left after the Kaioken ends, so it counts only
    /// outside Goku's Super Saiyan transformation and while no party record is Super Saiyan Goku (see
    /// [`Conditions::of_work_ram`]).
    kaioken_on: bool,
    /// The ending's flight with Gohan: game mode `$11` and Goku drawn from the flight's sprites, not crouching before
    /// Vegeta (see [`ENDING_FRAMES`]).
    ending_flight: bool,
    /// The frames since Goku's first Super Saiyan transformation started, while it goes on: the lightning of
    /// [`crate::lightning`] is drawn. The app's to say ([`Conditions::with_ssj_lightning`]); `None` from the work RAM.
    ssj_lightning: Option<u32>,
}

impl Conditions {
    /// The conditions of the work RAM `wram` (the 128 KiB of `$7E` and `$7F`): the Kaioken is on (`$0C36` bit 7), a party
    /// record has Super Saiyan Goku's unit (`$3B`), and the ending's flight is on screen: game mode `$11`, and not the frame
    /// of its loop ([`ENDING_FRAMES`]) from which Goku crouches before Vegeta with Vegeta in the ending's party. A `wram`
    /// too short to hold the bytes says no.
    ///
    /// The normal Goku uses the Kaioken while `$0C36` bit 7 is set, or while its multiplier ([`KAIOKEN_MULTIPLIER`]) is in
    /// use: the close-up of its first use comes before the bit. The multiplier outlives the Kaioken until the next turn,
    /// and Goku turns Super Saiyan in the turn he used it when a friend falls to Frieza then: the trigger ends the
    /// Kaioken (`$0C36` cleared, as the original's state says) and leaves the multiplier. So the multiplier is not the
    /// Kaioken in the transformation ([`SUPER_SAIYAN_PHASE`], his unit still `$3A`) nor once `$3B` is in the party:
    /// counted there, the frames of the transformation whose palette flashes the Kaioken's red (the original's own
    /// flashing) refused the Super Saiyan art of his close-up and of his powering up, and the game's sprites were drawn
    /// between the pictures.
    #[must_use]
    pub fn of_work_ram(wram: &[u8]) -> Self {
        let on = wram.get(KAIOKEN_STATE).is_some_and(|byte| byte & 0x80 != 0);
        let goku = (0..PARTY_SLOTS).any(|slot| {
            wram.get(PARTY_RECORDS + RECORD_STRIDE * slot + 1) == Some(&SUPER_SAIYAN_GOKU)
        });
        let transforming = wram.get(BATTLE_PHASE) == Some(&SUPER_SAIYAN_PHASE);
        let multiplier =
            !transforming && !goku && wram.get(KAIOKEN_MULTIPLIER).is_some_and(|&byte| byte != 0);
        let ending = wram.get(GAME_MODE).is_some_and(|&mode| mode & 0x7F == ENDING_MODE);
        let crouching = match (wram.get(ENDING_FRAMES..ENDING_FRAMES + 2), wram.get(ENDING_PARTY)) {
            (Some(&[low, high]), Some(&party)) => {
                u16::from_le_bytes([low, high]) >= ENDING_FLIGHT_END && party & ENDING_VEGETA != 0
            }
            _ => true,
        };
        Self {
            kaioken: on && goku,
            super_saiyan_goku: goku,
            kaioken_on: on || multiplier,
            ending_flight: ending && !crouching,
            ssj_lightning: None,
        }
    }

    /// These conditions with the frames since Goku's first Super Saiyan transformation started, while it goes on
    /// (`None`: it does not), which the work RAM alone does not say: the pictures of [`crate::lightning`] are drawn over
    /// the frame. A picture of other conditions is another picture, so a picture kept is not shown for this one.
    #[must_use]
    pub fn with_ssj_lightning(self, frames: Option<u32>) -> Self {
        Self { ssj_lightning: frames, ..self }
    }

    /// The frames since the transformation started, while it goes on (see [`Conditions::with_ssj_lightning`]).
    #[must_use]
    pub fn ssj_lightning(self) -> Option<u32> {
        self.ssj_lightning
    }

    fn holds(self, condition: ssd_mod::pose::Condition) -> bool {
        match condition {
            ssd_mod::pose::Condition::Kaioken => self.kaioken,
            ssd_mod::pose::Condition::Always => true,
        }
    }
}

/// The art of a pack, loaded as the frames need it, at the scale of the picture.
pub(crate) struct ArtState {
    pack: Arc<Pack>,
    /// What the capture of a frame's pixels reads of the pack (shared by the threads).
    lookup: Lookup,
    scale: usize,
    pictures: FxHashMap<u32, Option<Arc<Image>>>,
    objects: FxHashMap<u32, Option<Arc<Image>>>,
    recent: VecDeque<u32>,
    /// Art drawn in the colours of a palette other than the one it was made for, by what it is
    /// (a picture or an object), its index and the colours, and the bytes they take.
    variants: FxHashMap<VariantKey, Arc<Image>>,
    variant_bytes: usize,
    /// The pictures of the pack's pose variants at the scale, by the index of the variant and whether it is mirrored (see
    /// [`ArtState::pose_variant`]); `None` for a picture that cannot be read.
    pose_images: FxHashMap<(usize, bool), Option<Arc<Image>>>,
    /// The pixels of the last frame composed (by their index in it, `in_front_width` wide) a pose variant was drawn on
    /// where its pose is in front of what the frame shows: empty when no variant was drawn (see [`ArtState::uncover`]).
    in_front: Vec<bool>,
    in_front_width: usize,
    /// The conditions the pose variants are drawn under: those of the frame (see [`Conditions::of_work_ram`]).
    pub(crate) conditions: Conditions,
    /// The art of the background layers, by the index of the entry, kept within a budget (see
    /// [`bg_cache::BG_BUDGET`]).
    bg_images: bg_cache::BgImages,
    /// How the art is read: by the frames, or by a thread of its own (see [`ArtState::set_async`]).
    loading: load::Loading,
    /// What the frame being drawn cost so far, and the art it asked for and received (see [`ArtState::cost`]).
    cost: DrawCost,
    loads: LoadCounters,
    /// The background layers' LRU evictions counted when the frame being drawn started (see [`DrawCost::bg_evicted`]).
    bg_evicted_before: u64,
    /// The field maps' art: the source of the layer shown, and the tiles of art read at the
    /// picture's scale, by layer and tile.
    field_sources: FxHashMap<u32, Option<Arc<Image>>>,
    field_tiles: field::FieldTiles,
    /// The Mode 7 plane's art, by the index of the entry.
    plane_arts: FxHashMap<u32, Option<Arc<crate::mode7art::PlaneArt>>>,
    /// The plane the frame shows (see [`ArtState::begin_bg`]): the matrix of each line, by
    /// scanline, and the VRAM words the plane is made of.
    plane: Option<PlaneFrame>,
    /// The picture of the frame drawn again without BG1, when the plane is drawn from art with `replace`
    /// ([`ArtState::plane_replaces`], [`ArtState::set_plane_under`]): what is under the plane.
    plane_under: Option<crate::HdFrame>,
    /// The CGRAM at the start of the frame, the colours the pictures are drawn in.
    cgram: Vec<u16>,
    /// For the keys of partial poses seen: the object that holds it and where (see [`partial`]).
    partials: FxHashMap<u64, Option<(u32, i32, i32)>>,
    /// The number of poses of each unit of the index (see [`ArtState::unit_for`]).
    unit_sizes: std::sync::OnceLock<FxHashMap<String, u32>>,
    /// The coloured palettes of Super Vegeta's art (see [`ArtState::recolourable`]).
    super_vegeta: std::sync::OnceLock<Vec<Palette>>,
    /// The coloured palettes of Super Saiyan Goku's art (see [`ArtState::recolourable`]).
    super_saiyan_goku: std::sync::OnceLock<Vec<Palette>>,
    /// For the keys of poses seen that the index has no key for: the object of their shape.
    shapes: FxHashMap<u64, Option<u32>>,
    pub(crate) binder: Option<Binder<'static>>,
    /// Whether [`ArtStats::drawn`] is filled.
    pub(crate) measure: bool,
    /// The layers' art numbered by its colours, kept for the last pictures drawn (see [`ArtState::colourings`]), and
    /// whether the table transfers are made that way (`transfer_cache`).
    numbered: Vec<(Arc<BgImage>, Option<Arc<Colours>>)>,
    pub(crate) transfer_cache: bool,
    /// Draw the backgrounds' art alone (no poses, portraits or shadows): what an audit compares the
    /// layers' colours with the original's by.
    pub(crate) backgrounds_only: bool,
    /// The last answer of [`Lookup::choose`] and what it was asked.
    memo: Memo,
    /// The record of each pixel of the frame being drawn.
    pub(crate) pixels: Vec<ArtPixel>,
    /// The sprites (a bit for each OAM entry) of each pose of the last frame, as `stats.poses`.
    pose_sprites: Vec<u128>,
    /// The poses of the last frame that have no art, with the index of their entry in
    /// `stats.poses` (filled while the tally is on).
    dumps: Vec<(usize, PoseShape)>,
    /// Whether the frames are tallied instead of drawn (see [`ArtState::set_tally`]) and the last
    /// frame's tally.
    tally_on: bool,
    pub(crate) tally: FrameTally,
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// Whether the renderer records the pixels of the lines it draws.
    pub(crate) active: bool,
    /// The main loop is known to be in a field state (the map, a menu or a scene over it, a town),
    /// never a battle: a pose that has art made for a fighter is then still a figure of the field
    /// (see [`ArtState::find_poses_in`]). `None` when the caller does not know.
    pub(crate) field_frame: Option<bool>,
    pub(crate) stats: ArtStats,
}

impl ArtState {
    pub(crate) fn new(pack: Arc<Pack>) -> Self {
        let mut asset_pictures = vec![Vec::new(); pack.index.assets().len()];
        let by_id: FxHashMap<&str, usize> =
            pack.index.assets().iter().enumerate().map(|(at, a)| (a.id.as_str(), at)).collect();
        for (at, picture) in pack.art.pictures().iter().enumerate() {
            if let Some(&asset) = by_id.get(picture.asset.as_str()) {
                asset_pictures[asset].push(at as u32);
            }
        }
        let portrait_asset =
            pack.index.assets().iter().map(|a| a.id.starts_with("portrait/")).collect();
        Self {
            binder: Some(Binder::of_pack(Arc::clone(&pack))),
            lookup: Lookup {
                pack: Arc::clone(&pack),
                track: false,
                asset_pictures,
                portrait_asset,
                bg_classes: bgart::Classes::new(&pack.bg),
                bg_slots: Vec::new(),
            },
            pack,
            scale: 0,
            pictures: FxHashMap::default(),
            objects: FxHashMap::default(),
            recent: VecDeque::new(),
            variants: FxHashMap::default(),
            variant_bytes: 0,
            pose_images: FxHashMap::default(),
            in_front: Vec::new(),
            in_front_width: 0,
            conditions: Conditions::default(),
            bg_images: bg_cache::BgImages::default(),
            loading: load::Loading::default(),
            cost: DrawCost::default(),
            loads: LoadCounters::default(),
            bg_evicted_before: 0,
            field_sources: FxHashMap::default(),
            field_tiles: field::FieldTiles::default(),
            plane_arts: FxHashMap::default(),
            plane: None,
            plane_under: None,
            cgram: Vec::new(),
            partials: FxHashMap::default(),
            unit_sizes: std::sync::OnceLock::new(),
            super_vegeta: std::sync::OnceLock::new(),
            super_saiyan_goku: std::sync::OnceLock::new(),
            shapes: FxHashMap::default(),
            measure: false,
            numbered: Vec::new(),
            transfer_cache: true,
            backgrounds_only: false,
            memo: None,
            pixels: Vec::new(),
            pose_sprites: Vec::new(),
            tally_on: false,
            dumps: Vec::new(),
            tally: FrameTally::default(),
            width: 0,
            height: 0,
            active: false,
            field_frame: None,
            stats: ArtStats::default(),
        }
    }

    /// Turns the tally on: the pixels record what they show, [`ArtState::tally_frame`] counts
    /// them, and the pictures do not draw the art (only what would be drawn is counted, so a
    /// tour does not decode thousands of art files).
    pub(crate) fn set_tally(&mut self, on: bool) {
        self.tally_on = on;
        self.lookup.track = on;
    }

    pub(crate) fn tallying(&self) -> bool {
        self.tally_on
    }

    /// Whether the pack holds any art to draw.
    pub(crate) fn has_art(&self) -> bool {
        !self.pack.art.pictures().is_empty()
            || !self.pack.art.objects().is_empty()
            || !self.pack.bg.layer.is_empty()
            || !self.pack.maps.layer.is_empty()
            || !self.pack.mode7.plane.is_empty()
    }

    /// Starts a frame of `width` x `height` pixels drawn at `scale`.
    pub(crate) fn begin(&mut self, width: usize, height: usize, scale: usize) {
        if scale != self.scale {
            self.scale = scale;
            self.numbered.clear();
            self.pictures.clear();
            self.objects.clear();
            self.recent.clear();
            self.variants.clear();
            self.variant_bytes = 0;
            self.pose_images.clear();
            self.bg_images.clear();
            self.field_tiles.clear();
            self.plane_arts.clear();
        }
        self.field_frame = None;
        self.cost = DrawCost::default();
        self.loads = LoadCounters::default();
        self.bg_evicted_before = self.bg_images.evicted;
        // (a record is a hundred bytes: a frame's are megabytes, written by every worker)
        if self.pixels.len() == width * height {
            let band = crate::pool::band(height) * width;
            self.pixels.par_chunks_mut(band).for_each(|chunk| chunk.fill(ArtPixel::NONE));
        } else {
            self.pixels.clear();
            self.pixels.resize(width * height, ArtPixel::NONE);
        }
        self.width = width;
        self.height = height;
        self.stats = ArtStats::default();
        self.plane_under = None;
        self.pose_sprites.clear();
        self.dumps.clear();
        self.tally = FrameTally::default();
        self.begin_loading();
    }

    /// Whether the frame's plane is drawn from art with `replace` ([`crate::mode7art::Entry::replace`]): the frame is
    /// then drawn again without BG1 ([`without_bg1`]) for the picture under the plane ([`ArtState::set_plane_under`]).
    pub(crate) fn plane_replaces(&self) -> bool {
        self.plane.is_some()
            && self.lookup.bg_slots.iter().find(|slot| slot.is_plane()).is_some_and(|slot| {
                self.pack.mode7.plane.get(slot.entry as usize).is_some_and(|entry| entry.replace)
            })
    }

    /// The picture (the size of the frame's picture) of the frame drawn without BG1, for the plane of art with
    /// `replace` ([`ArtState::plane_replaces`]); counted in [`ArtStats::plane_under`].
    pub(crate) fn set_plane_under(&mut self, picture: crate::HdFrame) {
        self.plane_under = Some(picture);
        self.stats.plane_under += 1;
    }

    /// The background layers of the frame being drawn.
    pub(crate) fn bg_slots(&self) -> &[Slot] {
        &self.lookup.bg_slots
    }

    /// Finds the background layers of the pack that the frame `log` shows (by the content of
    /// their tiles), before its pixels are captured.
    pub(crate) fn begin_bg(&mut self, log: &FrameLog) {
        let started = std::time::Instant::now();
        let mut slots = if self.lookup.bg_classes.is_empty() {
            Vec::new()
        } else {
            self.lookup.bg_classes.find(&self.pack.bg, log)
        };
        if !self.lookup.bg_classes.is_empty() {
            self.stats.bg_unbound = self.lookup.bg_classes.unbound(&self.pack.bg, log, &slots);
        }
        self.plane = None;
        if let Some((at, plane)) = self.find_plane(log) {
            slots.insert(0, Slot::plane(at));
            self.plane = Some(plane);
        }
        self.cost.bg_slots = u32::try_from(slots.len()).unwrap_or(u32::MAX);
        self.cost.plane = self.plane.is_some();
        self.lookup.bg_slots = slots;
        self.cost.begin_bg += started.elapsed();
    }

    /// What the frame drawn since [`ArtState::begin`] cost so far: its stages, the files it read, its bindings.
    pub(crate) fn cost(&self) -> DrawCost {
        let mut cost = self.cost;
        self.loads.fill_cost(&mut cost);
        cost.bg_evicted =
            u32::try_from(self.bg_images.evicted - self.bg_evicted_before).unwrap_or(u32::MAX);
        cost
    }

    /// The Mode 7 plane of the pack that the frame shows, if any: found by the content of VRAM, and
    /// with the matrix of each of its lines. A frame that loads VRAM while it is displayed is not
    /// one plane.
    fn find_plane(&self, log: &FrameLog) -> Option<(u32, PlaneFrame)> {
        if self.pack.mode7.plane.is_empty() || !log.vram_writes.is_empty() {
            return None;
        }
        let shows = |regs: &LineRegs| {
            !regs.forced_blanking && regs.bg_mode == 7 && regs.main_bg[0] && !regs.direct_color
        };
        if !log.entries.iter().any(|entry| shows(&entry.regs)) {
            return None;
        }
        let at = self.pack.mode7.find_plane(&log.vram, &log.cgram_start)?;
        let mut lines = vec![None; usize::from(log.height) + 2];
        for entry in &log.entries {
            if shows(&entry.regs)
                && entry.from_pixel == 0
                && let Some(line) = lines.get_mut(usize::from(entry.scanline))
            {
                *line = Some(crate::mode7::Plane::of(&entry.regs));
            }
        }
        let words = log.vram.len().min(0x4000);
        Some((at as u32, PlaneFrame { lines, vram: log.vram[..words].to_vec() }))
    }

    /// The art of the plane of entry `at`, read whole (the art's own size) and kept.
    fn plane_art(&mut self, at: u32) -> Option<Arc<crate::mode7art::PlaneArt>> {
        if let Some(loaded) = self.plane_arts.get(&at) {
            return loaded.clone();
        }
        if self.loads_async() {
            self.want(load::ArtKey::Plane(at), 0);
            return None;
        }
        let loaded = self.load_plane_art(at);
        self.loaded_on_draw(1);
        self.plane_arts.insert(at, loaded.clone());
        loaded
    }

    fn load_plane_art(&self, at: u32) -> Option<Arc<crate::mode7art::PlaneArt>> {
        let _timer = self.loads.start(LoadKind::Plane);
        load::plane(&self.pack, at, true, &self.loads.bytes)
    }

    /// The art of background entry `at` at the picture's scale, with its source.
    fn bg_image(&mut self, at: u32) -> Option<Arc<BgImage>> {
        if let Some(loaded) = self.bg_images.get(at) {
            return loaded.clone();
        }
        if self.loads_async() {
            self.want(load::ArtKey::Bg(at), 0);
            return None;
        }
        let loaded = self.load_bg(at);
        self.loaded_on_draw(1);
        self.bg_images.reads += 1;
        self.bg_images.insert(at, loaded.clone(), bg_cache::BG_BUDGET);
        loaded
    }

    fn load_bg(&self, at: u32) -> Option<Arc<BgImage>> {
        let large = self.lookup.bg_slots.iter().any(|slot| slot.entry == at && slot.large);
        let _timer = self.loads.start(LoadKind::Bg);
        load::bg(&self.pack, at, large, self.scale, &self.loads.bytes)
    }

    /// The art of picture `at` of the index: 32 art pixels to a character, at the picture's scale.
    pub(crate) fn picture(&mut self, at: u32) -> Option<Arc<Image>> {
        if let Some(loaded) = self.pictures.get(&at) {
            return loaded.clone();
        }
        if self.loads_async() {
            self.want(load::ArtKey::Picture(at), 0);
            return None;
        }
        let image = self.load_picture(at);
        self.loaded_on_draw(1);
        self.pictures.insert(at, image.clone());
        image
    }

    /// The art of object `at` of the index at the picture's scale.
    pub(crate) fn object(&mut self, at: u32) -> Option<Arc<Image>> {
        if let Some(loaded) = self.objects.get(&at) {
            let loaded = loaded.clone();
            if let Some(place) = self.recent.iter().position(|&n| n == at) {
                self.recent.remove(place);
            }
            self.recent.push_back(at);
            return loaded;
        }
        if self.loads_async() {
            self.want(load::ArtKey::Object(at), 0);
            return None;
        }
        let image = self.load_object(at);
        self.loaded_on_draw(1);
        self.remember_object(at, image.clone());
        image
    }

    /// Records what the art passes need of the pixels `from..` of line `line` (1-based) that the
    /// renderer `r` has just resolved, in the row `x0 + x` of the record.
    pub(crate) fn capture_line(
        &mut self,
        r: &crate::Renderer,
        regs: &LineRegs,
        binder: &Binder<'_>,
        line: usize,
        from: usize,
    ) {
        for x in from..crate::WIDTH {
            let plan_as = |layer, palette| r.plan_as(regs, x, layer, palette);
            let layer_colour = |bg| r.bg_colour(regs, bg, x);
            let layer_index = |bg| r.bg_index(regs, bg, x);
            let probe = Probe {
                rendered: &r.main.rendered[x],
                plan_as: &plan_as,
                layer_colour: &layer_colour,
                layer_index: &layer_index,
                backdrop: r.cgram[0],
                window_x: r.origin + x as i32,
                line,
                world: None,
            };
            let pixel = self.pixel(&probe, regs, &r.cgram, binder);
            self.pixels[(line - 1) * self.width + x] = pixel;
        }
    }

    /// What the art passes need of one resolved pixel.
    pub(crate) fn pixel(
        &mut self,
        probe: &Probe<'_>,
        regs: &LineRegs,
        cgram: &[u16; 256],
        binder: &Binder<'_>,
    ) -> ArtPixel {
        self.lookup.pixel(
            probe,
            regs,
            cgram,
            binder,
            &mut self.memo,
            &mut self.stats.portrait_pixels_without_art,
        )
    }

    /// The part of the state that captures the pixels of rows in parallel: the lookup, and the
    /// record of the frame to split by rows.
    pub(crate) fn capture_parts(&mut self) -> (&Lookup, &mut [ArtPixel]) {
        (&self.lookup, &mut self.pixels)
    }

    /// Sets the record of the frame aside before the frame is drawn again without the sprites the poses hide (the
    /// picture under them): that drawing captures its own record ([`ArtState::end_under`] takes what the poses'
    /// pixels show under them from it). The counts of the frame are kept as they are.
    pub(crate) fn begin_under(&mut self) -> UnderCapture {
        let blank = vec![ArtPixel::NONE; self.pixels.len()];
        UnderCapture {
            first: std::mem::replace(&mut self.pixels, blank),
            stats: self.stats.clone(),
        }
    }

    /// Puts the record of the frame back after the picture under the poses was drawn (see [`ArtState::begin_under`]):
    /// when it was (`drawn`), each pixel the hidden sprites (`hidden`) won in rows `rows` takes what the backgrounds
    /// are there in that picture ([`merge_under`]).
    pub(crate) fn end_under(
        &mut self,
        saved: UnderCapture,
        drawn: bool,
        hidden: u128,
        rows: Option<(usize, usize)>,
    ) {
        let under = std::mem::replace(&mut self.pixels, saved.first);
        self.stats = saved.stats;
        if drawn {
            merge_under(&mut self.pixels, &under, self.width, hidden, rows);
        }
    }

    /// The background slots of the last picture, one row per slot: the index entry, its BG, the
    /// registers its layer was shown with (mode, map base, char base, size) and whether it is a
    /// field map's layer.
    pub(crate) fn bg_slot_report(&self) -> Vec<(u32, u8, u8, u16, u16, u8, bool)> {
        self.lookup
            .bg_slots
            .iter()
            .map(|s| (s.entry, s.bg, s.mode, s.map_base, s.char_base, s.size, s.field))
            .collect()
    }

    /// The box `w` x `h` at `(x, y)` of the last picture, one row per pixel: the layer on top (an
    /// object `O`, a BG's `U`nder/`L`ayer/`H`idden, or `B`), the picture it is part of (`None` when
    /// it is no picture's) and its CGRAM row. It says which part of a portrait is drawn from a
    /// picture and which is left as the game's own pixels.
    pub(crate) fn picture_map(
        &self,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
    ) -> Vec<(u8, Option<u32>, u8)> {
        let width = self.width;
        let mut out = Vec::with_capacity(w * h);
        for j in 0..h {
            for i in 0..w {
                let (x, y) = (x + i as i32, y + j as i32);
                let Some(pixel) = (x >= 0 && y >= 0)
                    .then(|| (y as usize) * width + (x as usize))
                    .and_then(|at| self.pixels.get(at))
                else {
                    out.push((b'?', None, 0));
                    continue;
                };
                let kind = if pixel.oam != crate::NO_OBJECT {
                    b'O'
                } else {
                    match pixel.bg.kind {
                        bgart::Kind::Under => b'U',
                        bgart::Kind::Layer => b'L',
                        bgart::Kind::Hidden => b'H',
                        bgart::Kind::Object => b'B',
                    }
                };
                let picture = if pixel.picture == NO_PICTURE { None } else { Some(pixel.picture) };
                out.push((kind, picture, pixel.row));
            }
        }
        out
    }

    /// The poses of the frame `log` whose art the index holds, with the art's object index.
    pub(crate) fn find_poses(&mut self, log: &FrameLog) -> Vec<(Pose, u32)> {
        self.find_poses_in(log, &poses::View::window())
    }

    /// [`ArtState::find_poses`] for the sprites of `view`: the window alone, or a wide view of the
    /// field with its margins and the objects the original drops.
    pub(crate) fn find_poses_in(&mut self, log: &FrameLog, view: &poses::View) -> Vec<(Pose, u32)> {
        let started = std::time::Instant::now();
        let found = self.find_poses_of(log, view);
        self.cost.poses = u32::try_from(found.len()).unwrap_or(u32::MAX);
        self.cost.find_poses += started.elapsed();
        found
    }

    fn find_poses_of(&mut self, log: &FrameLog, view: &poses::View) -> Vec<(Pose, u32)> {
        self.cgram.clear();
        self.cgram.extend_from_slice(&log.cgram_start);
        if self.pack.art.objects().is_empty() || self.backgrounds_only {
            return Vec::new();
        }
        let sprites = poses::sprites_in(log, view);
        let overscan = poses::overscan_sprites_in(log, view);
        if sprites.is_empty() && overscan.is_empty() {
            return Vec::new();
        }
        let mut found = poses::poses_in(log, &sprites, view);
        // The fighters the game draws with the other palettes (an enemy of a formation), when the
        // index has art for them, before the effects the same sprites make
        // (only in a frame of fighters: a field group of such a palette is the field's)
        let battle = self.field_frame != Some(true)
            && found.iter().any(|pose| pose.fighter && !self.candidates(pose).is_empty());
        // Outside a battle, a field group with no art is read again (see `poses::refine_field`)
        if !battle {
            found = poses::refine_field(found, &sprites, log, view, &mut |pose| {
                !self.candidates(pose).is_empty() || self.shaped(pose).is_some()
            });
            found = poses::join_band(found, &sprites, &overscan, log, view, &mut |pose| {
                !self.candidates(pose).is_empty() || self.shaped(pose).is_some()
            });
        }
        let extra: Vec<Pose> = if battle {
            poses::fighter_candidates(&sprites)
                .into_iter()
                .filter(|pose| !self.candidates(pose).is_empty())
                .collect()
        } else {
            Vec::new()
        };
        if !extra.is_empty() {
            let at = found.iter().position(|pose| !pose.fighter).unwrap_or(found.len());
            found.splice(at..at, extra);
        }
        // The fighters drawn as BG tiles (the end slots of a formation of five), when the index has
        // art for their pixels
        // The speed lines of a cutscene that reach under line 224 (a frame of 239 lines shows them):
        // the pieces under it are poses of their own, found only by the exact pixels the index has
        // art for (or all of them, when the tour looks for what has no art)
        if !battle {
            // (the pieces a figure of the field took with it are drawn by its art)
            let joined =
                found.iter().filter(|pose| pose.field).fold(0_u128, |all, pose| all | pose.sprites);
            for mut pose in poses::overscan_effects(log, &overscan) {
                if pose.sprites & !joined == 0 {
                    continue;
                }
                // (a cluster that also holds pieces a figure took in the band is placed on the
                // others: the figure's art draws those, the effect's the rest)
                pose.excluded = pose.sprites & joined;
                if self.tally_on || !self.candidates(&pose).is_empty() {
                    found.push(pose);
                }
            }
        }
        let bg_fighters = self.bg_fighters(log);
        if !bg_fighters.is_empty() {
            found.extend(bg_fighters);
        }
        // The visible pixels of each sprite, and those of them that belong to a picture (a portrait
        // drawn as sprites); every worker counts a stretch of the frame's records
        let stretch = self.pixels.len().div_ceil(rayon::current_num_threads().max(1)).max(8192);
        let (shown, pictured) = self
            .pixels
            .par_chunks(stretch)
            .fold(
                || ([0_u32; 128], [0_u32; 128]),
                |(mut shown, mut pictured), pixels| {
                    for pixel in pixels.iter().filter(|p| p.oam != crate::NO_OBJECT) {
                        let at = usize::from(pixel.oam) & 127;
                        shown[at] += 1;
                        pictured[at] += u32::from(pixel.picture != NO_PICTURE);
                    }
                    (shown, pictured)
                },
            )
            .reduce(
                || ([0_u32; 128], [0_u32; 128]),
                |mut a, b| {
                    for (x, y) in a.0.iter_mut().zip(b.0) {
                        *x += y;
                    }
                    for (x, y) in a.1.iter_mut().zip(b.1) {
                        *x += y;
                    }
                    a
                },
            );
        let sum_of = |counts: &[u32; 128], pose: &Pose| {
            (0..128).filter(|&at| pose.sprites >> at & 1 != 0).map(|at| counts[at]).sum::<u32>()
        };
        let seen = |pose: &Pose| sum_of(&shown, pose);
        // Sprites that show a picture are that picture's, whatever fighter's or effect's silhouette
        // they share: a pose is never made of them but by the exact pixels it has art for
        let is_picture = |pose: &Pose| {
            let (all, own) = (sum_of(&shown, pose), sum_of(&pictured, pose));
            own > 0 && own * 2 >= all
        };
        let mut taken = 0_u128;
        let mut stats = Vec::new();
        let mut all_masks: Vec<(u128, Kind)> = Vec::new();
        for pose in &found {
            all_masks.push((pose.sprites, Kind::of(pose)));
            stats.push(PoseStat {
                fighter: pose.fighter,
                field: pose.field,
                shadow: pose.shadow,
                sprites: pose.sprites & !pose.excluded,
                excluded: pose.excluded,
                picture: is_picture(pose),
                matched: false,
                recoloured: false,
                visible: seen(pose),
                opaque: pose.arr.iter().filter(|&&c| c != 0).count() as u32,
                stripy: stripy_permille(pose),
                key: pose.key,
                palette: pose.palette,
                source: ArtSource::None,
                art: None,
                org: pose.org,
                size: (pose.w, pose.h),
                look: if self.measure {
                    poses::region_digest(&sprites, pose.org, pose.w, pose.h)
                } else {
                    0
                },
            });
        }
        let rows: Vec<[[u8; 3]; 16]> = found
            .iter()
            .map(|pose| {
                if pose.bg {
                    Self::bg_palette_row(log, pose.palette)
                } else {
                    Self::object_palette_row(log, pose.palette)
                }
            })
            .collect();
        let free = |pose: &Pose, taken: u128| (pose.sprites & !pose.excluded) & taken == 0;
        let mut placed: Vec<Option<(Pose, u32)>> = vec![None; found.len()];
        let mut sources: Vec<ArtSource> = vec![ArtSource::None; found.len()];
        // First the poses whose pixels the index has a key for (the art made for the exact pose),
        // in the order of the frame; then the ground shadows; then the poses found by their shape
        // or as part of another pose (a fade-in, a cut by the screen's edge)
        // Outside a battle the poses with the most pixels come first (a field group before an
        // effect's cluster of as many): the whole figure takes its sprites before a piece of it
        // does (a cloud's 42 x 13 corner that an effect's art fits must not take them from the
        // group, nor the pieces a group is cut in from the whole figure that art was made for), so
        // a figure is drawn from the same art in every frame whatever way its sprites are grouped
        let mut order: Vec<usize> = (0..found.len()).collect();
        if !battle {
            let opaque = |n: usize| found[n].arr.iter().filter(|&&c| c != 0).count();
            order.sort_by_key(|&n| (std::cmp::Reverse(opaque(n)), !found[n].field));
        }
        for n in order {
            let pose = &found[n];
            if pose.shadow || !free(pose, taken) {
                continue;
            }
            let unit = self.unit_for(pose, &rows[n]);
            let (best, mirrored, refused) = self.exact_art(pose, &rows[n], unit.as_ref());
            if let Some(at) = best.map(|at| self.ending_goku(at)) {
                taken |= pose.sprites & !pose.excluded;
                let placed_at = if mirrored { at | MIRROR } else { at };
                placed[n] = Some((self.placed(pose, at, &rows[n], false), placed_at));
                sources[n] = if mirrored { ArtSource::Mirror } else { ArtSource::Exact };
            } else if refused {
                // The pose keeps the game's sprite: its sprites are not another pose's (the field's group of
                // the same sprites, Super Vegeta's dash as `field-frieza-force-000`), nor drawn by a shape
                taken |= pose.sprites & !pose.excluded;
            }
        }
        for (n, pose) in found.iter().enumerate() {
            if pose.shadow && free(pose, taken) {
                taken |= pose.sprites;
                placed[n] = Some((pose.clone(), SHADOW));
            }
        }
        for (n, pose) in found.iter().enumerate() {
            if placed[n].is_some() || pose.shadow || !free(pose, taken) || is_picture(pose) {
                continue;
            }
            // A pose of the sprites of a colour variant (their tiles are the same, the key is
            // not: an effect's reads its colours) is the pose of the same shape
            let (found_pose, at, shaped) = match self.shaped(pose) {
                Some(at) => (pose.clone(), at, true),
                // Only a pose the capture would have left out is a part of another one: a fade-in's
                // stripes or a sprite the screen's edge cuts. A whole pose that has no art (a spike
                // of Porunga's shoulder) is missing, never drawn as some other fighter's piece
                None if (pose.fighter || pose.cut)
                    && stats[n].opaque >= partial::MIN_OPAQUE
                    && (pose.cut || stats[n].stripy >= STRIPY_PARTIAL) =>
                {
                    match self.holding(pose, &rows[n]) {
                        Some((held, at)) => (held, at, false),
                        None => continue,
                    }
                }
                None => continue,
            };
            // (the art of the same shape or holding the pose is drawn in the frame's colours too, else the
            // pose keeps the game's sprite; art holding a piece must have the piece's colours, see
            // `partial::draws`)
            let at = self.held_override(at, shaped, pose, &found_pose, &rows[n]).unwrap_or(at);
            let palette = &self.pack.art.objects()[at as usize].palette;
            let held_wrong = !shaped && !partial::draws(palette, &rows[n], pose);
            if held_wrong || !self.recolourable(&found_pose, at, &rows[n]) {
                taken |= pose.sprites;
                continue;
            }
            taken |= pose.sprites;
            let at = self.ending_goku(at);
            placed[n] = Some((self.placed(&found_pose, at, &rows[n], shaped), at));
            sources[n] = if shaped { ArtSource::Shaped } else { ArtSource::Held };
        }
        // Art that cannot be loaded (a missing or unreadable file) draws nothing, and its sprites
        // are not hidden: they stay as the game draws them
        let wanted: Vec<(Pose, u32)> =
            placed.iter().flatten().filter(|(_, at)| *at != SHADOW).cloned().collect();
        self.prefetch(&std::collections::BTreeSet::new(), &wanted);
        let mut matches: Vec<(Pose, u32)> = Vec::new();
        for (n, slot) in placed.into_iter().enumerate() {
            if let Some((mut pose, at)) = slot {
                if at != SHADOW {
                    // (a pose whose art or pose variant is still being read is drawn as the game draws it)
                    // (both asked for in the same frame)
                    let drawn = self.object(at).is_some();
                    let asked = self.loads_async() && self.variant_asked(at);
                    if !drawn || asked {
                        continue;
                    }
                }
                stats[n].matched = true;
                stats[n].recoloured = pose.recolour.is_some();
                if at != SHADOW {
                    stats[n].source = sources[n];
                    stats[n].art = Some(real(at) as u32);
                    pose.source = sources[n];
                }
                matches.push((pose, at));
            }
        }
        // The art of the layers draws under a fighter drawn as BG tiles that the art draws
        for (pose, _) in matches.iter().filter(|(pose, at)| pose.bg && *at != SHADOW) {
            for n in (0..pose.arr.len()).filter(|&n| pose.arr[n] != 0) {
                let (fx, fy) =
                    (pose.org.0 + (n % pose.w) as i32, pose.org.1 + (n / pose.w) as i32 - 1);
                let at = fy as usize * self.width + fx as usize;
                if let Some(pixel) = self.pixels.get_mut(at)
                    && pixel.bg.kind == bgart::Kind::Under
                {
                    pixel.bg.kind = bgart::Kind::Layer;
                }
            }
        }
        if self.tally_on {
            for (n, pose) in found.iter().enumerate() {
                if !stats[n].matched {
                    let shape = PoseShape {
                        key: pose.key,
                        fighter: pose.fighter,
                        field: pose.field,
                        shadow: pose.shadow,
                        fate: Fate::Missing,
                        w: pose.w,
                        h: pose.h,
                        arr: pose.arr.clone(),
                        rgba: pose.rgba.clone(),
                        org: pose.org,
                        palette: pose.palette,
                        row: rows[n],
                    };
                    self.dumps.push((n, shape));
                }
            }
        }
        // Pixels by what became of the sprite that shows them: art beats missing beats partial
        let mut masks = [[0_u128; 5]; 3];
        let mut drawn_by_any = 0_u128;
        for (pose, _) in &matches {
            drawn_by_any |= pose.sprites;
        }
        for (stat, (mask, kind)) in stats.iter().zip(&all_masks) {
            let kind = *kind as usize;
            let class =
                if stat.matched || (kind == Kind::Field as usize && mask & !drawn_by_any == 0) {
                    0
                } else if stat.opaque >= 40 && stat.stripy < STRIPY_PARTIAL {
                    1
                } else {
                    2
                };
            masks[kind][class] |= mask;
            masks[kind][3] |= mask;
            if stat.recoloured {
                masks[kind][4] |= mask;
            }
        }
        let width = self.width.max(1);
        let cut_without_art = found
            .iter()
            .zip(&stats)
            .filter(|(pose, stat)| {
                pose.field && (pose.cut || pose.at_vertical_edge()) && !stat.matched
            })
            .fold(0_u128, |all, (pose, _)| all | pose.sprites);
        for (at, pixel) in self.pixels.iter().enumerate().filter(|(_, p)| p.oam != crate::NO_OBJECT)
        {
            let bit = 1_u128 << pixel.oam;
            let mut any = [0_u128; 5];
            for (kind, mask) in masks.iter().enumerate() {
                if mask[3] & bit == 0 {
                    continue;
                }
                let counts = match kind {
                    0 => &mut self.stats.fighters,
                    1 => &mut self.stats.effects,
                    _ => &mut self.stats.field,
                };
                count_pixel(counts, mask, bit, pixel);
                for (all, one) in any.iter_mut().zip(mask) {
                    *all |= one;
                }
            }
            // A sprite no pose holds (the capture left groups of fewer than 40 pixels out: what is
            // left of an object cut by the view's edge) is left out too
            if (cut_without_art & bit != 0 || any[3] & bit == 0) && pixel.picture == NO_PICTURE {
                self.stats.sprites_cut += 1;
                continue;
            }
            let region = if view.in_margin((at % width) as i32) {
                &mut self.stats.sprites_margin
            } else {
                &mut self.stats.sprites_window
            };
            count_pixel(region, &any, bit, pixel);
        }
        self.pose_sprites = all_masks.iter().map(|&(mask, _)| mask).collect();
        self.stats.poses = stats;
        matches
    }

    /// Draws the art of the frame over `hd`, the picture of `frame` (the 1x frame with its
    /// sprites): the pictures, then the poses `found`. `x_offset` is where the window starts in
    /// the frame, `clip` the part of it the poses may draw on (left, top, right, bottom).
    pub(crate) fn compose(
        &mut self,
        hd: &mut crate::HdFrame,
        frame: &crate::Frame,
        found: &[(Pose, u32)],
        x_offset: i32,
        clip: (i32, i32, i32, i32),
    ) {
        let started = std::time::Instant::now();
        self.compose_art(hd, frame, found, x_offset, clip);
        self.cost.art_compose += started.elapsed();
    }

    /// The sprites of the poses of `found` drawn from the art made for their exact pixels (or for them mirrored) that
    /// mostly show a picture but not only: a portrait the art of a cluster holds with other sprites (see
    /// `compose_art`), as a mask of OAM entries. A pose drawn from art found by its shape or holding it is not one.
    fn portraits_among_others(&self, found: &[(Pose, u32)]) -> u128 {
        let (mut shown, mut pictured) = ([0_u32; 128], [0_u32; 128]);
        for pixel in self.pixels.iter().filter(|p| p.oam != crate::NO_OBJECT) {
            let at = usize::from(pixel.oam) & 127;
            shown[at] += 1;
            pictured[at] += u32::from(pixel.picture != NO_PICTURE);
        }
        let mut portraits = 0_u128;
        let exact = |pose: &Pose| matches!(pose.source, ArtSource::Exact | ArtSource::Mirror);
        for (pose, _) in found.iter().filter(|(pose, at)| *at != SHADOW && exact(pose)) {
            let sprites = pose.sprites & !pose.excluded;
            let sum = |counts: &[u32; 128]| {
                (0..128).filter(|&at| sprites >> at & 1 != 0).map(|at| counts[at]).sum::<u32>()
            };
            let (all, own) = (sum(&shown), sum(&pictured));
            if own > 0 && own * 2 >= all && own < all {
                portraits |= sprites;
            }
        }
        portraits
    }

    fn compose_art(
        &mut self,
        hd: &mut crate::HdFrame,
        frame: &crate::Frame,
        found: &[(Pose, u32)],
        x_offset: i32,
        clip: (i32, i32, i32, i32),
    ) {
        let scale = self.scale;
        self.in_front.clear();
        self.in_front_width = frame.width;
        let hidden = found.iter().fold(0_u128, |all, (pose, _)| all | pose.sprites);
        self.repair_portraits();
        let ground_shadows = bgshadow::find(frame, &mut self.pixels);
        self.draw_backgrounds(hd, hidden, x_offset);
        if self.backgrounds_only {
            return;
        }
        let mut images = FxHashMap::default();
        let mut recoloured = std::collections::BTreeSet::new();
        let wanted_rows: std::collections::BTreeSet<(u32, u8)> = self
            .pixels
            .iter()
            .filter(|p| p.picture != NO_PICTURE)
            .map(|p| (p.picture, p.row))
            .collect();
        let wanted: std::collections::BTreeSet<u32> =
            wanted_rows.iter().map(|&(at, _)| at).collect();
        self.prefetch(&wanted, found);
        for (at, row) in wanted_rows {
            if let Some(image) = self.picture(at) {
                let made = self.picture_variant(at, row, Arc::clone(&image));
                if !Arc::ptr_eq(&made, &image) {
                    recoloured.insert((at, row));
                }
                images.insert((at, row), made);
            }
        }
        self.stats.picture_pixels =
            draw_pictures(&mut hd.rgba, scale, self.width, &self.pixels, &images);
        self.stats.picture_recoloured = if recoloured.is_empty() {
            0
        } else {
            let drawn = |p: &&ArtPixel| p.picture != NO_PICTURE;
            self.pixels
                .iter()
                .filter(drawn)
                .filter(|p| recoloured.contains(&(p.picture, p.row)))
                .count() as u64
        };
        // The shadows go under the fighters
        let shadows: Vec<&Pose> =
            found.iter().filter(|(_, at)| *at == SHADOW).map(|(pose, _)| pose).collect();
        let others = found
            .iter()
            .filter(|(_, at)| *at != SHADOW)
            .fold(0_u128, |all, (pose, _)| all | pose.sprites);
        draw_shadows(&mut hd.rgba, scale, frame, &self.pixels, &shadows, others, x_offset, clip);
        bgshadow::draw(&mut hd.rgba, scale, frame, &self.pixels, &ground_shadows, others, clip);
        let mut placed: Vec<Placed<'_>> = Vec::new();
        // The poses a pose variant replaces: drawn by the variant, after the others
        let mut replaced: Vec<(&Pose, Arc<Image>, i32)> = Vec::new();
        // The sprites of the poses drawn from a picture of a folder of `art-override\` that adds poses
        // (`super-vegeta\`, `goku\`, `ending-goku\`: `Object::override_only`): such a picture need not cover the
        // sprites' pixels (Super Vegeta's has no shoulder pads), and where it does not, the art of a pose behind them
        // is drawn; the poses are then drawn from back to front (see `draw_objects`)
        let mut open = 0_u128;
        for (pose, at) in found.iter().filter(|(_, at)| *at != SHADOW) {
            if let Some((image, margin)) = self.pose_variant(*at) {
                replaced.push((pose, image, margin));
                continue;
            }
            let object = &self.pack.art.objects()[real(*at)];
            let (luma, override_only) = (object.luma, object.override_only());
            if let Some(image) = self.object(*at) {
                let image = match &pose.recolour {
                    Some(variant) => self.object_variant(pose, *at, image, variant),
                    None => image,
                };
                if override_only {
                    open |= pose.sprites & !pose.excluded;
                }
                placed.push((pose, image, luma));
            }
        }
        let measure = self.measure;
        let drawn = draw_objects(
            &mut hd.rgba,
            scale,
            frame,
            &self.pixels,
            &placed,
            x_offset,
            clip,
            measure,
            open,
        );
        // A portrait the art of a cluster holds with other sprites (the opening's Goku with the speed lines beside him:
        // the art made for each frame of the lines has a Goku of its own, four different ones) is drawn from its
        // picture again over that art, the other sprites from the cluster's: the portrait is the same in every frame.
        // A cluster of only a portrait's sprites keeps the art made for it (the field's faces)
        let portraits = if wanted.is_empty() { 0 } else { self.portraits_among_others(found) };
        if portraits != 0 {
            let only: Vec<ArtPixel> = self
                .pixels
                .iter()
                .map(|p| {
                    let held = p.oam != crate::NO_OBJECT && portraits >> (p.oam & 127) & 1 != 0;
                    if held { *p } else { ArtPixel { picture: NO_PICTURE, ..*p } }
                })
                .collect();
            draw_pictures(&mut hd.rgba, scale, self.width, &only, &images);
        }
        if !replaced.is_empty() {
            self.in_front.resize(frame.width * frame.height, false);
        }
        for (pose, image, margin) in &replaced {
            let x_offset = if pose.bg { 0 } else { x_offset };
            let origin = (pose.org.0 + x_offset - margin, pose.org.1 + ROW_OF_CAPTURE - margin);
            let sprites = pose.sprites & !pose.excluded;
            draw_variant(
                &mut hd.rgba,
                scale,
                frame,
                &self.pixels,
                sprites,
                pose.priority,
                image,
                origin,
                clip,
                &mut self.in_front,
            );
        }
        if measure {
            self.stats.drawn = placed
                .iter()
                .zip(drawn)
                .map(|(&(pose, _, luma), drawn)| DrawnPose {
                    key: pose.key,
                    fighter: pose.fighter,
                    field: pose.field,
                    luma,
                    drawn,
                })
                .collect();
        }
    }

    /// Draws the art of the background layers over `hd`, where the original showed a layer, the
    /// backdrop or an object that an HD pose hides (`hidden`), and counts the pixels of battle
    /// backgrounds and the ones drawn.
    fn draw_backgrounds(&mut self, hd: &mut crate::HdFrame, hidden: u128, x_offset: i32) {
        self.stats.bg_pixels = self.pixels.iter().filter(|p| p.bg.battle).count() as u64;
        let slots = self.lookup.bg_slots.clone();
        if slots.is_empty() || self.tally_on {
            return;
        }
        let images: Vec<Option<Arc<BgImage>>> = slots
            .iter()
            .enumerate()
            .map(|(j, slot)| {
                if slot.is_plane() {
                    None
                } else if slot.field {
                    self.field_image(slot, j)
                } else {
                    self.bg_image(slot.entry)
                }
            })
            .collect();
        if let Some(j) = slots.iter().position(Slot::is_plane)
            && let Some(plane) = self.plane.take()
        {
            let size = hd.rgba.len();
            if let Some(art) = self.plane_art(slots[j].entry) {
                draw::draw_plane(
                    &mut hd.rgba,
                    self.scale,
                    self.width,
                    &self.pixels,
                    &draw::PlaneDraw {
                        frame: &plane,
                        cgram: &self.cgram,
                        art: &art,
                        x_offset,
                        slot: j,
                        hidden,
                        under: self
                            .plane_under
                            .as_ref()
                            .map(|under| &under.rgba[..])
                            .filter(|under| under.len() == size),
                    },
                );
            }
            self.plane = Some(plane);
        }
        let fills: Vec<bool> = slots.iter().map(|slot| slot.fill).collect();
        let colourings = self.colourings(&images, hidden);
        self.stats.bg_art = draw::draw_backgrounds(
            &mut hd.rgba,
            self.scale,
            self.width,
            &self.pixels,
            &images,
            &colourings,
            &fills,
            hidden,
        );
    }

    /// How the colours of each slot's art are transferred to the frame's (see [`crate::transfer`]). A table transfer of a
    /// whole picture is applied to the picture's own colours, once each (see [`Colours`]), the picture numbered once
    /// (`numbered`); a fade, a tiled art and a picture with too many colours are applied to each colour as it is drawn.
    /// With `transfer_cache` off every table transfer is applied so: the reference `bench-hd --check` compares with.
    fn colourings(&mut self, images: &[Option<Arc<BgImage>>], hidden: u128) -> Vec<Colouring> {
        let mut colourings = Vec::with_capacity(images.len());
        for (slot, image) in images.iter().enumerate() {
            let Some(image) = image else {
                colourings.push(Colouring::Same);
                continue;
            };
            let pairs = draw::pairs_of(image, slot, &self.pixels, hidden);
            if pairs.is_identity() {
                colourings.push(Colouring::Same);
                continue;
            }
            let transfer = pairs.transfer();
            let numbered = match &transfer {
                Transfer::Table(_) if self.transfer_cache => self.numbered_colours(image),
                _ => None,
            };
            colourings.push(match numbered {
                Some(colours) => {
                    let transferred = colours.transferred(&transfer);
                    Colouring::Numbered { colours, transferred }
                }
                None => Colouring::Each(transfer),
            });
        }
        colourings
    }

    /// The colours of a picture of a layer's art, numbered (see [`Colours`]): `None` for a tiled art and for a picture
    /// with too many colours. The last pictures numbered are kept, so a picture is numbered once while it is drawn.
    fn numbered_colours(&mut self, image: &Arc<BgImage>) -> Option<Arc<Colours>> {
        if let Some((_, colours)) =
            self.numbered.iter().find(|(source, _)| Arc::ptr_eq(source, image))
        {
            return colours.clone();
        }
        let colours = match &image.art {
            BgArt::Whole(art) => Colours::of(art).map(Arc::new),
            BgArt::Tiled(_) => None,
        };
        if self.numbered.len() >= NUMBERED_PICTURES {
            self.numbered.remove(0);
        }
        self.numbered.push((Arc::clone(image), colours.clone()));
        colours
    }

    /// Numbers the layers' art by its colours (`on`, the default), or applies each table transfer to each colour as it
    /// is drawn (off: the reference that `bench-hd --check` compares with; the pictures are the same).
    pub(crate) fn set_transfer_cache(&mut self, on: bool) {
        self.transfer_cache = on;
        self.numbered.clear();
    }

    /// Picture `at` in the colours of CGRAM row `row` (see [`ArtPixel::row`]) when the picture was
    /// made for other colours: its art recoloured, or `image` itself.
    fn picture_variant(&mut self, at: u32, row: u8, image: Arc<Image>) -> Arc<Image> {
        let base = usize::from(row) * 16;
        let Some(words) = self.cgram.get(base..base + 16) else { return image };
        let colours: [[u8; 3]; 16] = std::array::from_fn(|n| poses::rgb555(words[n]));
        let palette = self.pack.art.pictures()[at as usize].palette;
        self.variant((true, at, 0), image, &palette, &colours)
    }

    /// Object `at` drawn for `pose` in the colours of `variant`: recoloured from the palette it
    /// was made for, or from the palette its art has when the pose says which index is which.
    fn object_variant(
        &mut self,
        pose: &Pose,
        at: u32,
        image: Arc<Image>,
        variant: &recolour::Variant,
    ) -> Arc<Image> {
        let object = &self.pack.art.objects()[real(at)];
        if !variant.derive {
            let palette = object.palette;
            return self.variant((false, at, 0), image, &palette, &variant.row);
        }
        if let Some(found) = self.variants.get(&(false, at, pose.key, variant.row)) {
            return Arc::clone(found);
        }
        let palette = recolour::derive_palette(&image, &pose.arr, pose.w, pose.h, self.scale);
        self.variant((false, at, pose.key), image, &palette, &variant.row)
    }

    /// The art `image` (of picture or object `key.1`, made for `palette`) in the colours `row`,
    /// kept while the pictures that need it are drawn.
    fn variant(
        &mut self,
        key: (bool, u32, u64),
        image: Arc<Image>,
        palette: &Palette,
        row: &[[u8; 3]; 16],
    ) -> Arc<Image> {
        let key = (key.0, key.1, key.2, *row);
        if let Some(found) = self.variants.get(&key) {
            return Arc::clone(found);
        }
        if !recolour::differs(palette, row) {
            return image;
        }
        let made = Arc::new(recolour::recolour(&image, palette, row));
        if self.variant_bytes > VARIANT_BYTES {
            self.variants.clear();
            self.variant_bytes = 0;
        }
        self.variant_bytes += made.rgba.len();
        self.variants.insert(key, Arc::clone(&made));
        made
    }

    /// Loads the art a frame needs and the cache lacks, all at once on the thread pool: the first
    /// sight of a pose costs the decoding of its file, and a frame can bring several.
    fn prefetch(&mut self, pictures: &std::collections::BTreeSet<u32>, found: &[(Pose, u32)]) {
        let new_pictures: Vec<u32> =
            pictures.iter().copied().filter(|at| !self.pictures.contains_key(at)).collect();
        let new_objects: std::collections::BTreeSet<u32> = found
            .iter()
            .map(|(_, at)| *at)
            .filter(|at| *at != SHADOW && !self.objects.contains_key(at))
            .collect();
        if self.loads_async() {
            // (asked for in the frame's order, before the frame asks for them one at a time)
            for &at in &new_pictures {
                self.want(load::ArtKey::Picture(at), 0);
            }
            for &at in &new_objects {
                self.want(load::ArtKey::Object(at), 0);
            }
            return;
        }
        if new_pictures.len() + new_objects.len() < 2 {
            return;
        }
        self.loaded_on_draw(new_pictures.len() + new_objects.len());
        let (loaded_pictures, loaded_objects): (Vec<_>, Vec<_>) = rayon::join(
            || new_pictures.par_iter().map(|&at| (at, self.load_picture(at))).collect(),
            || {
                new_objects
                    .iter()
                    .copied()
                    .collect::<Vec<u32>>()
                    .par_iter()
                    .map(|&at| (at, self.load_object(at)))
                    .collect()
            },
        );
        for (at, image) in loaded_pictures {
            self.pictures.insert(at, image);
        }
        for (at, image) in loaded_objects {
            self.remember_object(at, image);
        }
    }

    fn load_picture(&self, at: u32) -> Option<Arc<Image>> {
        let _timer = self.loads.start(LoadKind::Picture);
        load::picture(&self.pack, at, self.scale, &self.loads.bytes)
    }

    fn load_object(&self, at: u32) -> Option<Arc<Image>> {
        let _timer = self.loads.start(LoadKind::Object);
        load::object(&self.pack, at, self.scale, &self.loads.bytes)
    }

    /// `cells` (the text cells of the frame last composed) without the pixels a pose variant was drawn on in front of them,
    /// when there are any: the HD text fills a cell's pixels with its ground and draws its glyph there after the art, so a
    /// variant's margin over a cell the game shows past the sprites (the white of a BG3 aura or blast behind the fighter,
    /// cells of the blank glyph) was painted over, the hair cut along the cells' edges. A cell in front of the pose (a
    /// window over it) keeps its pixels. `None` when no pixel of `cells` is taken out.
    /// Per pixel of the last picture, whether it is drawn from one of the pack's pictures (a portrait, a face).
    pub(crate) fn picture_pixels(&self) -> Vec<bool> {
        self.pixels.iter().map(|pixel| pixel.picture != NO_PICTURE).collect()
    }

    pub(crate) fn uncover(
        &self,
        cells: &[crate::font::TextCell],
    ) -> Option<Vec<crate::font::TextCell>> {
        if self.in_front.is_empty() {
            return None;
        }
        let width = self.in_front_width as i32;
        let height = (self.in_front.len() / self.in_front_width.max(1)) as i32;
        let covered = |x: i32, y: i32| {
            (0..width).contains(&x)
                && (0..height).contains(&y)
                && self.in_front[(y * width + x) as usize]
        };
        let mut changed = false;
        let uncovered = cells
            .iter()
            .map(|cell| {
                let mut cell = *cell;
                for bit in 0..64_i32 {
                    if cell.mask >> bit & 1 != 0 && covered(cell.x + bit % 8, cell.y + bit / 8) {
                        cell.mask &= !(1 << bit);
                        changed = true;
                    }
                }
                cell
            })
            .collect();
        changed.then_some(uncovered)
    }

    /// The picture of a pose variant of `object` at the scale (see [`load::pose_variant`]).
    fn load_variant(
        &self,
        variant: &PoseVariant,
        object: &crate::art_index::Object,
        mirrored: bool,
    ) -> Option<Arc<Image>> {
        let _timer = self.loads.start(LoadKind::Variant);
        load::pose_variant(&self.pack, variant, object, mirrored, self.scale, &self.loads.bytes)
    }

    /// The picture of the pose variant the pack gives the object `at` while the conditions of the frame hold, and its margin
    /// in game pixels: of the object's variants that hold, one for a specific condition before an `always` one
    /// ([`ssd_mod::pose::drawn`]). A data pack's variants ([`Pack::variants`]) come before the pack's own bigger art
    /// ([`Pack::pose_art`]): the own art is drawn only where no data pack's variant of the object holds, or its picture is not
    /// drawn (see [`ArtState::load_variant`]). `None` when no variant holds for the object, or no picture is drawn: the object
    /// is then drawn as it is.
    fn pose_variant(&mut self, at: u32) -> Option<(Arc<Image>, i32)> {
        match self.pose_variant_art(at) {
            VariantArt::Drawn(image, margin) => Some((image, margin)),
            VariantArt::Asked | VariantArt::None => None,
        }
    }

    /// Whether the picture of the pose variant the object `at` is drawn from is asked of the reading thread and not read
    /// yet (see [`ArtState::set_async`]): the pose is then drawn as the game draws it, as a pose whose art is not read.
    fn variant_asked(&mut self, at: u32) -> bool {
        matches!(self.pose_variant_art(at), VariantArt::Asked)
    }

    /// [`ArtState::pose_variant`], and whether the picture is asked of the reading thread.
    fn pose_variant_art(&mut self, at: u32) -> VariantArt {
        let pack = Arc::clone(&self.pack);
        let object = &pack.art.objects()[real(at)];
        let mirrored = at & MIRROR != 0;
        // The variants are numbered: the data packs' first, then the pack's own
        let own = pack.variants.len();
        for (variants, first) in [(&pack.variants, 0), (&pack.pose_art, own)] {
            let holding: Vec<usize> = (0..variants.len())
                .filter(|&index| {
                    let variant = &variants[index];
                    variant.art == object.id && self.conditions.holds(variant.when)
                })
                .collect();
            let Some(chosen) =
                ssd_mod::pose::drawn(holding.iter().map(|&index| variants[index].when))
            else {
                continue;
            };
            let index = holding[chosen];
            let variant = &variants[index];
            let margin = variant.margin as i32;
            let loaded = match self.pose_images.get(&(first + index, mirrored)) {
                Some(loaded) => loaded.clone(),
                None if self.loads_async() => {
                    self.want(load::ArtKey::Variant(first + index, mirrored), at);
                    return VariantArt::Asked;
                }
                None => {
                    let loaded = self.load_variant(variant, object, mirrored);
                    self.loaded_on_draw(1);
                    self.pose_images.insert((first + index, mirrored), loaded.clone());
                    loaded
                }
            };
            if let Some(image) = loaded {
                return VariantArt::Drawn(image, margin);
            }
        }
        VariantArt::None
    }

    /// Keeps `image` as the art of object `at`, the least recently used one making room.
    fn remember_object(&mut self, at: u32, image: Option<Arc<Image>>) {
        if self.recent.len() >= OBJECT_CACHE
            && let Some(oldest) = self.recent.pop_front()
        {
            self.objects.remove(&oldest);
        }
        self.recent.push_back(at);
        self.objects.insert(at, image);
    }

    /// The pose of the index that holds the partial fighter `pose` (see [`partial::containing`]),
    /// as the pose to draw: the holding pose's silhouette placed where the partial one's pixels
    /// fall on it, with the art's object index. The answer is kept by the partial pose's key.
    fn holding(&mut self, pose: &Pose, row: &[[u8; 3]; 16]) -> Option<(Pose, u32)> {
        let found = match self.partials.get(&pose.key) {
            Some(found) => *found,
            None => {
                let found = partial::containing(&self.pack.art, row, pose);
                if self.partials.len() >= PARTIAL_CACHE {
                    self.partials.clear();
                }
                self.partials.insert(pose.key, found);
                found
            }
        };
        let (at, dx, dy) = found?;
        let object = &self.pack.art.objects()[at as usize];
        // A sprite the screen's edge cuts shows a good part of its pose: a few pixels that fit
        // into some big art (a speed line's end) are not a piece of it
        if stripy_permille(pose) < STRIPY_PARTIAL {
            let shown = pose.arr.iter().filter(|&&c| c != 0).count() as u32;
            if shown * 100 < object.opaque() * CUT_MIN_SHARE {
                return None;
            }
        }
        let (w, h) = (usize::from(object.width), usize::from(object.height));
        let arr = (0..h * w).map(|n| u8::from(object.opaque_at(n % w, n / w))).collect();
        Some((Pose { arr, w, h, org: (pose.org.0 - dx, pose.org.1 - dy), ..pose.clone() }, at))
    }

    /// `pose` as drawn from object `at`: with the colours of `row` when the art was made for
    /// another palette (see [`ArtState::variant_row`]).
    fn placed(&self, pose: &Pose, at: u32, row: &[[u8; 3]; 16], shaped: bool) -> Pose {
        let mut placed = pose.clone();
        placed.recolour = self.variant_row(at, row, shaped);
        placed
    }

    /// The colours to draw object `at` in when it was made for a palette other than `row`.
    /// `shaped`: the object was found by its shape, not its key, so its colours are not known to
    /// differ and its palette may be unknown (it is then read from the pose).
    fn variant_row(&self, at: u32, row: &[[u8; 3]; 16], shaped: bool) -> Option<recolour::Variant> {
        let object = &self.pack.art.objects()[at as usize];
        if object.luma {
            return None;
        }
        let derive = object.palette.mask == 0;
        let variant = recolour::Variant { row: *row, derive };
        if derive {
            shaped.then_some(variant)
        } else {
            recolour::differs(&object.palette, row).then_some(variant)
        }
    }

    /// Whether object `at` can draw `pose` in the colours of `row`: not when the recolouring would
    /// give one colour where the row has two ([`recolour::merges`]) and `row` is Super Vegeta's
    /// palette, or Super Saiyan Goku's on a pose the pack has no Super Saiyan art for (the palette of
    /// some art of theirs, [`SUPER_VEGETA_MARK`], [`SUPER_SAIYAN_GOKU_MARK`], with every colour the
    /// object uses). The game draws Super Vegeta in some poses (his energy wave, his Exploding Wave,
    /// a kick that throws him) with Vegeta's tiles in his own palette, the hair (14, 15) yellow and
    /// orange, and a piece of Super Saiyan Goku's head at the screen's edge in the hidden fight with
    /// tiles the pack has only normal art for (`noartfix-f884cde40d`); that art is of the normal
    /// forms, where the hair is the black of the outline (1), and recoloured it showed them with
    /// black hair. Such a
    /// pose keeps the game's sprite, its own pixels and colours, as a pose with no art does. Where
    /// the pack has Super Saiyan art for the pose, [`Self::unit_for`] chooses between the sets.
    /// A picture of `art-override\super-vegeta\` (patch 0049,
    /// [`crate::art_index::Object::super_vegeta_only`]) is drawn only when `row` is Super Vegeta's palette: the colours of
    /// some art of his with every colour the pose uses and his hair, so that Vegeta's colours (black hair) never take
    /// it, even in a pose whose art shows no hair. His picture of an effect's pose (palette [`game_over_super_vegeta`]) is
    /// drawn only when `row` is that palette, every colour.
    /// A picture of `art-override\goku\` (patch 0051, [`crate::art_index::Object::normal_goku_only`]) is drawn only when
    /// the hair of `row` ([`GOKU_HAIR`]) is not Super Saiyan Goku's and no two of its colours become two in `row`
    /// ([`recolour::merges`]): the picture's hair is the black of its outline, so it is drawn in the normal Goku's black
    /// hair, and in the Kaioken's colours where the outline is as red as the hair, but where the hair is red and the outline
    /// is not the pose keeps the game's sprite as before (patch 0050).
    fn recolourable(&self, pose: &Pose, at: u32, row: &[[u8; 3]; 16]) -> bool {
        let object = &self.pack.art.objects()[at as usize];
        if object.normal_goku_only() {
            return !self.super_saiyan_hair(row) && !recolour::merges(&object.palette, row);
        }
        if self.normal_goku_in_super_saiyan_art(pose, at, row) {
            return false;
        }
        if object.super_vegeta_only() {
            // (his picture of the effect's pose of GAME OVER: in that scene's palette of his only)
            if object.palette == game_over_super_vegeta() {
                return object.palette.distance(row) == 0;
            }
            let wanted = object.palette.mask | SUPER_VEGETA_HAIR;
            return self
                .super_vegeta()
                .iter()
                .any(|palette| palette.mask & wanted == wanted && palette.distance(row) == 0);
        }
        if object.luma || !recolour::merges(&object.palette, row) {
            return true;
        }
        let mask = object.palette.mask;
        let of_row = |palettes: &[Palette]| {
            palettes.iter().any(|palette| palette.mask & mask == mask && palette.distance(row) == 0)
        };
        let goku = || {
            of_row(self.super_saiyan_goku())
                && !self
                    .candidates(pose)
                    .iter()
                    .any(|&(at, _)| self.unit_of(at).contains(SUPER_SAIYAN_GOKU_MARK))
        };
        !(of_row(self.super_vegeta()) || goku())
    }

    /// The coloured palettes of Super Vegeta's art in the index.
    fn super_vegeta(&self) -> &[Palette] {
        self.super_vegeta.get_or_init(|| self.palettes_of(SUPER_VEGETA_MARK))
    }

    /// Whether `at` is Super Saiyan Goku's art drawn for the normal Goku using the Kaioken (patch 0050): the Kaioken is on
    /// (`$0C36` bit 7, or its multiplier `$01EF` in use, as in the close-up of its first use), the hair of `row`
    /// ([`GOKU_HAIR`]) is not the Super Saiyan's (black in his own palette, red in the Kaioken's), and the pack has no art
    /// of the normal Goku for the pose (or its mirror image). The pack has only Super Saiyan art for some poses the game
    /// draws for both (the whole body powering up, `battle-goku-ssj-020`, is the Kaioken's close-up as well as the Super
    /// Saiyan's): recoloured, its hair and the aura stay gold, and Goku using the Kaioken (and the Kaioken with the
    /// Kamehameha) was drawn as the Super Saiyan. Such a pose keeps the game's sprite, as a pose with no art does.
    /// Without the Kaioken nothing changes (Goku turning Super Saiyan before Frieza is drawn from that art in the frames
    /// whose palette still has his black hair, as before), nor in the Super Saiyan's hair. Where the pack has art of both
    /// for the pose, [`Self::unit_for`] chooses between the sets.
    fn normal_goku_in_super_saiyan_art(&self, pose: &Pose, at: u32, row: &[[u8; 3]; 16]) -> bool {
        if !self.conditions.kaioken_on || !self.unit_of(at).contains(SUPER_SAIYAN_GOKU_MARK) {
            return false;
        }
        let normal =
            |unit: &str| unit.contains(GOKU_MARK) && !unit.contains(SUPER_SAIYAN_GOKU_MARK);
        !self.super_saiyan_hair(row)
            && !self.candidates(pose).iter().any(|&(at, _)| normal(&self.unit_of(at)))
    }

    /// Whether the hair of `row` ([`GOKU_HAIR`]) is the colours of some art of Super Saiyan Goku.
    fn super_saiyan_hair(&self, row: &[[u8; 3]; 16]) -> bool {
        self.super_saiyan_goku().iter().any(|palette| {
            palette.mask & GOKU_HAIR == GOKU_HAIR
                && Palette { mask: GOKU_HAIR, ..*palette }.distance(row) == 0
        })
    }

    /// The coloured palettes of Super Saiyan Goku's art in the index.
    fn super_saiyan_goku(&self) -> &[Palette] {
        self.super_saiyan_goku.get_or_init(|| self.palettes_of(SUPER_SAIYAN_GOKU_MARK))
    }

    /// The coloured palettes of the art whose id has `mark`.
    fn palettes_of(&self, mark: &str) -> Vec<Palette> {
        let objects = self.pack.art.objects();
        objects
            .iter()
            .filter(|object| {
                object.id.contains(mark) && object.palette.mask != 0 && !object.override_only()
            })
            .map(|object| object.palette)
            .collect()
    }

    /// The object of the index with the silhouette of `pose` (the same box and the same opaque
    /// pixels), when the pose is one a palette variant could be: big enough, of several
    /// colours, and not a plain block. The answer is kept by the pose's key.
    fn shaped(&mut self, pose: &Pose) -> Option<u32> {
        let memo = pose.key ^ if pose.field { 0x5EED_F1E1_D000_0000 } else { 0 };
        if let Some(found) = self.shapes.get(&memo) {
            return *found;
        }
        let opaque = pose.arr.iter().filter(|&&c| c != 0).count();
        let mut seen = 0_u16;
        for &c in pose.arr.iter().filter(|&&c| c != 0) {
            seen |= 1 << (c & 15);
        }
        let found = if opaque >= SHAPE_MIN_OPAQUE
            && opaque < pose.arr.len()
            && seen.count_ones() >= 3
            && u16::try_from(pose.w).is_ok()
            && u16::try_from(pose.h).is_ok()
        {
            let art = &self.pack.art;
            art.objects_shaped(pose.w as u16, pose.h as u16, opaque as u32).iter().copied().find(
                |&at| {
                    let object = &art.objects()[at as usize];
                    object.field() == pose.field
                        && (0..pose.w * pose.h)
                            .all(|n| object.opaque_at(n % pose.w, n / pose.w) == (pose.arr[n] != 0))
                },
            )
        } else {
            None
        };
        if self.shapes.len() >= PARTIAL_CACHE {
            self.shapes.clear();
        }
        self.shapes.insert(memo, found);
        found
    }

    /// The art made for exactly the pixels of `pose`, or for them mirrored left to right (drawn
    /// mirrored: the second answer), among the objects of the pose's kind that can be drawn in the
    /// colours of `row` ([`Self::recolourable`]): the art of `unit` when one is given, in the
    /// palette closest to `row`; the art of the pose's own facing before the art of the other one.
    /// The third answer: there is art of `unit` (or of any unit) for the pose, none of which can be
    /// drawn in those colours.
    fn exact_art(
        &self,
        pose: &Pose,
        row: &[[u8; 3]; 16],
        unit: Option<&String>,
    ) -> (Option<u32>, bool, bool) {
        if let Some(at) = self.ending_space_vegeta(pose, row) {
            return (Some(at), false, false);
        }
        let of_unit: Vec<(u32, bool)> = self
            .candidates(pose)
            .into_iter()
            .filter(|&(at, _)| unit.is_none_or(|unit| self.unit_of(at) == *unit))
            .collect();
        let best =
            of_unit.iter().copied().filter(|&(at, _)| self.recolourable(pose, at, row)).min_by_key(
                |&(at, mirrored)| {
                    let palette = &self.pack.art.objects()[at as usize].palette;
                    (u8::from(mirrored), palette.distance(row), u8::from(palette.mask == 0))
                },
            );
        match best {
            Some((at, mirrored)) => (Some(at), mirrored, false),
            None => (None, false, !of_unit.is_empty()),
        }
    }

    /// The picture of `art-override\ending-goku\` drawn in place of object `at` (Goku's art for the pose, see
    /// [`crate::art_index::Object::ending_goku_only`]) while the ending's flight with Gohan is on screen
    /// ([`Conditions::of_work_ram`]): his torn clothes there, the art as before everywhere else. `at` itself in any other
    /// frame, for art that has no such picture, and without the folder.
    fn ending_goku(&self, at: u32) -> u32 {
        if !self.conditions.ending_flight {
            return at;
        }
        let objects = self.pack.art.objects();
        let object = &objects[at as usize];
        self.pack
            .art
            .objects_with(object.key)
            .iter()
            .copied()
            .find(|&other| {
                let other = &objects[other as usize];
                other.ending_goku_only()
                    && other.field() == object.field()
                    && other.id.strip_suffix(crate::art_index::ENDING_GOKU_ONLY)
                        == Some(object.id.as_str())
            })
            .unwrap_or(at)
    }

    /// The picture of `art-override\super-vegeta\` for the field group of [`ENDING_SPACE_VEGETA`] (Super Vegeta in
    /// space), when `row` is Super Vegeta's palette ([`Self::recolourable`]). `None` for any other pose, in other colours
    /// (Vegeta's), or without the picture: the pose is then found as before.
    fn ending_space_vegeta(&self, pose: &Pose, row: &[[u8; 3]; 16]) -> Option<u32> {
        if !pose.field || pose.key != ENDING_SPACE_VEGETA {
            return None;
        }
        let objects = self.pack.art.objects();
        self.pack.art.objects_with(pose.key).iter().copied().find(|&at| {
            objects[at as usize].super_vegeta_only() && self.recolourable(pose, at, row)
        })
    }

    /// The picture of `art-override\super-vegeta\` (patch 0049) to draw `piece` from in place of object `at`, the art of the
    /// same shape (`shaped`) or the art of the whole pose `held` that holds it, when `at` cannot draw it in the colours of
    /// `row` and the picture of `at`'s key can: Super Vegeta vanishing in his teleport (`battle-vegeta-804`) is drawn in
    /// stripes the index has no key for, held by Vegeta's art of the pose (black hair where he has two colours), and the
    /// stripes kept the game's sprite because his picture is found by its key only. `None` where `at` can draw the piece
    /// (nothing that was drawn from art changes) or no such picture can. `at` an effect's art (palette mask 0, recoloured
    /// from the sprite's own colours, so it draws in any): the picture is drawn wherever it can be, Vegeta flying over
    /// GAME OVER (`gallery-fx-1118`, with the pads) in his palette of that scene ([`game_over_super_vegeta`]), the art
    /// as before in any other.
    fn held_override(
        &self,
        at: u32,
        shaped: bool,
        piece: &Pose,
        held: &Pose,
        row: &[[u8; 3]; 16],
    ) -> Option<u32> {
        let objects = self.pack.art.objects();
        let drawable = |at: u32| {
            (shaped || partial::draws(&objects[at as usize].palette, row, piece))
                && self.recolourable(held, at, row)
        };
        let effect = {
            let object = &objects[at as usize];
            !object.luma && object.palette.mask == 0
        };
        if drawable(at) && !effect {
            return None;
        }
        self.pack
            .art
            .objects_with(objects[at as usize].key)
            .iter()
            .copied()
            .find(|&other| objects[other as usize].super_vegeta_only() && drawable(other))
    }

    /// The objects of the pose's kind made for its pixels, and for them mirrored (`true`).
    fn candidates(&self, pose: &Pose) -> Vec<(u32, bool)> {
        let index = &self.pack.art;
        // (the poses of `art-override\ending-goku\` are drawn only in place of their art, see `Self::ending_goku`)
        let of_kind = |&&at: &&u32| {
            let object = &index.objects()[at as usize];
            object.field() == pose.field && !object.ending_goku_only()
        };
        let mut found: Vec<(u32, bool)> =
            index.objects_with(pose.key).iter().filter(of_kind).map(|&at| (at, false)).collect();
        if let Some(mirror) = pose.mirror_key().filter(|&mirror| mirror != pose.key) {
            found.extend(index.objects_with(mirror).iter().filter(of_kind).map(|&at| (at, true)));
        }
        found
    }

    /// The unit an object's art is of: its id without the number of the pose (`battle-saibaman-004`
    /// is of `battle-saibaman`).
    fn unit_of(&self, at: u32) -> String {
        let id = &self.pack.art.objects()[at as usize].id;
        match id.rsplit_once('-') {
            Some((unit, number)) if number.bytes().all(|b| b.is_ascii_digit()) => unit.to_owned(),
            _ => id.clone(),
        }
    }

    /// The unit whose art draws a pose whose pixels (or their mirror image) more than one unit has
    /// art for. The units of one set of pixels are the same sprite in other colours (the three
    /// Saibamen of the game) and each was drawn on its own, so they differ in the details: the
    /// fighters of a formation are drawn from the unit with the most poses in the index, in the
    /// colours of their own palettes, whatever their colours, and so are the frames of an
    /// animation. While Super Saiyan Goku is in the party, a pose that both his normal and his
    /// Super Saiyan art have is drawn from the Super Saiyan art, unless the colours of `row` are
    /// nearer the normal art's (see [`Self::goku_colours`]): the normal art has the same
    /// pixels, in the normal palette, where his hair is black, and the scenes of the ending draw
    /// him so (black hair, the normal palette) with his Super Saiyan unit still in the party, but
    /// for the pose of [`GOKU_HOLDING_GOHAN`], drawn from the Super Saiyan art whatever its colours
    /// (a choice of this branch);
    /// without him, from the Super Saiyan art when the colours of `row` are nearer its art's (Goku
    /// turning Super Saiyan before Frieza: the close-up `battle-goku-068` in the Super Saiyan palette
    /// before `$3B` is in a party record), else from the normal art, however many poses the Super
    /// Saiyan set has: the palette decides, the party record breaks a tie. Otherwise a
    /// unit with art for the pose made for exactly the colours of `row` is drawn from: two units of
    /// the same pixels can be two characters, not two colours of one (Vegeta and Super Vegeta), and
    /// the art of the one with more poses, recoloured, cannot give the other's colours back where
    /// its palette has one colour for several (Vegeta's black hair, eyes and outline). When several
    /// units have art for the exact colours, the one of them with the most poses; when none has,
    /// the unit with the most poses. `None` when one unit (or none) has art for the pose.
    ///
    /// A picture of `art-override\super-vegeta\` (patch 0049) or `art-override\goku\` (patch 0051) for the pose that can be
    /// drawn in the colours of `row` (Super Vegeta's; the normal Goku's black hair, see [`Self::recolourable`]) is the
    /// unit drawn from; one that cannot is left out, so the choice is as it was without it.
    fn unit_for(&self, pose: &Pose, row: &[[u8; 3]; 16]) -> Option<String> {
        let mut candidates = self.candidates(pose);
        let objects = self.pack.art.objects();
        if let Some(&(at, _)) = candidates.iter().find(|&&(at, _)| {
            objects[at as usize].override_only() && self.recolourable(pose, at, row)
        }) {
            return Some(self.unit_of(at));
        }
        candidates.retain(|&(at, _)| !objects[at as usize].override_only());
        let mut units: Vec<String> = candidates.iter().map(|&(at, _)| self.unit_of(at)).collect();
        units.sort();
        units.dedup();
        if units.len() < 2 {
            return None;
        }
        let super_saiyan = |unit: &String| unit.contains(SUPER_SAIYAN_GOKU_MARK);
        let normal = |unit: &String| unit.contains(GOKU_MARK) && !super_saiyan(unit);
        if units.iter().any(super_saiyan) && units.iter().any(normal) {
            // Any set of Goku's art (the battle poses, the close-ups of the special attacks) that has a Super Saiyan set for
            // these pixels is drawn from it while he is in the party and the frame does not colour him as the normal Goku,
            // and from the normal set otherwise.
            // Goku holding Gohan before Super Vegeta is the Super Saiyan whatever the colours: a choice of this branch
            // (see `GOKU_HOLDING_GOHAN`)
            let holding_gohan = candidates
                .iter()
                .any(|&(at, _)| self.pack.art.objects()[at as usize].key == GOKU_HOLDING_GOHAN);
            let colours = self.goku_colours(&candidates, row);
            let drawn_super_saiyan = if self.conditions.super_saiyan_goku {
                holding_gohan || colours != Some(std::cmp::Ordering::Less)
            } else {
                // Without `$3B` only colours strictly nearer the Super Saiyan art's make him one
                colours == Some(std::cmp::Ordering::Greater)
            };
            if drawn_super_saiyan {
                return units.into_iter().find(super_saiyan);
            }
            units.retain(|unit| !super_saiyan(unit));
        }
        // A palette of no colour is 0 from every row: it is not art made for these colours
        let objects = self.pack.art.objects();
        let exact: Vec<String> = candidates
            .iter()
            .filter(|&&(at, _)| {
                let palette = &objects[at as usize].palette;
                palette.mask != 0 && palette.distance(row) == 0
            })
            .map(|&(at, _)| self.unit_of(at))
            .collect();
        if units.iter().any(|unit| exact.contains(unit)) {
            units.retain(|unit| exact.contains(unit));
        }
        let sizes = self.unit_sizes();
        units.into_iter().max_by_key(|unit| {
            (sizes.get(unit).copied().unwrap_or(0), std::cmp::Reverse(unit.clone()))
        })
    }

    /// How `row` colours a pose that both Goku's normal and his Super Saiyan art have: `Less` when the colours of the
    /// normal art's palette are strictly nearer `row` than those of the Super Saiyan art's (the normal Goku),
    /// `Greater` when the Super Saiyan art's are strictly nearer, `Equal` when they are as near. Only the
    /// indices every coloured palette of the two sets uses are compared, so a colour one set has and the other does
    /// not (the Super Saiyan's index 11, absent from `gallery-party-goku-u00-056`) weighs on neither side, and the
    /// colours the sets share (skin, outline) weigh the same on both: what decides is where they differ, the hair (14
    /// and 15, black for the normal Goku, yellow for the Super Saiyan) and the gi. The game draws Super Saiyan Goku in
    /// the normal palette in the scenes of the ending (the flight with Gohan, Goku holding him before Super Vegeta),
    /// with unit `$3B` still in a party record; in a fight it draws him in the Super Saiyan palette, and as he turns
    /// Super Saiyan before Frieza, before `$3B` is in a party record. `None` when either set has no coloured palette
    /// for the pose: the party record decides then, as it does when the two are as near.
    fn goku_colours(
        &self,
        candidates: &[(u32, bool)],
        row: &[[u8; 3]; 16],
    ) -> Option<std::cmp::Ordering> {
        let objects = self.pack.art.objects();
        let coloured: Vec<(bool, Palette)> = candidates
            .iter()
            .filter_map(|&(at, _)| {
                let unit = self.unit_of(at);
                let palette = objects[at as usize].palette;
                (unit.contains(GOKU_MARK) && palette.mask != 0)
                    .then(|| (unit.contains(SUPER_SAIYAN_GOKU_MARK), palette))
            })
            .collect();
        let shared = coloured.iter().fold(u16::MAX, |mask, (_, palette)| mask & palette.mask);
        let nearest = |super_saiyan: bool| {
            coloured
                .iter()
                .filter(|&&(of, _)| of == super_saiyan)
                .map(|(_, palette)| {
                    Palette { mask: palette.mask & shared, ..*palette }.distance(row)
                })
                .min()
        };
        match (nearest(false), nearest(true)) {
            (Some(normal), Some(super_saiyan)) => Some(normal.cmp(&super_saiyan)),
            _ => None,
        }
    }

    /// The number of poses of each unit of the index.
    fn unit_sizes(&self) -> &rustc_hash::FxHashMap<String, u32> {
        self.unit_sizes.get_or_init(|| {
            let mut sizes: rustc_hash::FxHashMap<String, u32> = rustc_hash::FxHashMap::default();
            for at in 0..self.pack.art.objects().len() {
                *sizes.entry(self.unit_of(at as u32)).or_default() += 1;
            }
            sizes
        })
    }

    fn bg_palette_row(log: &FrameLog, palette: u8) -> [[u8; 3]; 16] {
        let mut row = [[0_u8; 3]; 16];
        for (index, color) in row.iter_mut().enumerate() {
            *color = poses::rgb555(log.cgram_start[usize::from(palette & 7) * 16 + index]);
        }
        row
    }

    /// The fighters the frame draws as four-bit BG tiles in front of the art's layers (BG1: the end
    /// slots of a formation of five enemies are drawn that way): the clusters of such pixels (at
    /// most 2 pixels apart) read as poses by their palette indexes, kept when the art index has
    /// art for their pixels or their mirror image. Their `org` is in frame coordinates.
    fn bg_fighters(&self, log: &FrameLog) -> Vec<Pose> {
        let (width, height) = (self.width, self.pixels.len() / self.width.max(1));
        let candidate = |at: usize| {
            let p = &self.pixels[at];
            p.bg4 && p.oam == crate::NO_OBJECT && p.bg.kind == bgart::Kind::Under
        };
        let points: Vec<usize> = (0..self.pixels.len()).filter(|&at| candidate(at)).collect();
        if points.len() < 40 {
            return Vec::new();
        }
        let mut label: FxHashMap<usize, usize> = FxHashMap::default();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &start in &points {
            if label.contains_key(&start) {
                continue;
            }
            let id = groups.len();
            let mut stack = vec![start];
            label.insert(start, id);
            let mut members = Vec::new();
            while let Some(at) = stack.pop() {
                members.push(at);
                let (x, y) = ((at % width) as i32, (at / width) as i32);
                for dy in -2_i32..=2 {
                    for dx in -2_i32..=2 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                            continue;
                        }
                        let near = ny as usize * width + nx as usize;
                        if candidate(near) && !label.contains_key(&near) {
                            label.insert(near, id);
                            stack.push(near);
                        }
                    }
                }
            }
            groups.push(members);
        }
        let mut out = Vec::new();
        for members in groups {
            if members.len() < 40 {
                continue;
            }
            let x0 = members.iter().map(|at| at % width).min().unwrap_or(0);
            let x1 = members.iter().map(|at| at % width).max().unwrap_or(0) + 1;
            let y0 = members.iter().map(|at| at / width).min().unwrap_or(0);
            let y1 = members.iter().map(|at| at / width).max().unwrap_or(0) + 1;
            let (w, h) = (x1 - x0, y1 - y0);
            let mut arr = vec![0_u8; w * h];
            for &at in &members {
                let p = &self.pixels[at];
                let (col, row) = (usize::from(p.bg_pos & 7), usize::from(p.bg_pos >> 3 & 7));
                let base = usize::from(p.bg_char);
                let Some(data) = log.vram.get(base..base + 16) else { continue };
                let bit = 7 - col;
                let mut color = 0_u8;
                for plane in 0..2_usize {
                    let word = data[row + 8 * plane];
                    color |= u8::from((word >> bit) & 1 != 0) << (2 * plane);
                    color |= u8::from((word >> (bit + 8)) & 1 != 0) << (2 * plane + 1);
                }
                arr[(at / width - y0) * w + (at % width - x0)] = color;
            }
            let palette = self.pixels[members[0]].bg_palette;
            let pose = poses::bg_pose(arr, w, h, (x0 as i32, y0 as i32 + 1), palette);
            if !self.candidates(&pose).is_empty() {
                out.push(pose);
            }
        }
        out
    }

    fn object_palette_row(log: &FrameLog, palette: u8) -> [[u8; 3]; 16] {
        let mut row = [[0_u8; 3]; 16];
        for (index, color) in row.iter_mut().enumerate() {
            *color = poses::rgb555(log.cgram_start[128 + usize::from(palette) * 16 + index]);
        }
        row
    }
}

/// The record of the pixels of some rows of a frame, filled by one thread: the rows of
/// [`ArtState::pixels`] that a segment of lines draws.
pub(crate) struct Rows<'a> {
    lookup: &'a Lookup,
    pixels: &'a mut [ArtPixel],
    width: usize,
    /// The 1-based line of the first row of `pixels`.
    first_line: usize,
    memo: Memo,
    /// Characters of portraits that had no picture.
    pub(crate) portrait: u64,
}

impl<'a> Rows<'a> {
    /// The background layers of the frame (see [`ArtState::bg_slots`]).
    pub(crate) fn bg_slots(&self) -> &[Slot] {
        &self.lookup.bg_slots
    }

    pub(crate) fn new(
        lookup: &'a Lookup,
        pixels: &'a mut [ArtPixel],
        width: usize,
        first_line: usize,
    ) -> Self {
        Self { lookup, pixels, width, first_line, memo: None, portrait: 0 }
    }

    /// Records pixel `x` of line `line` (1-based), resolved as `rendered`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn put(
        &mut self,
        line: usize,
        x: usize,
        probe: &Probe<'_>,
        regs: &LineRegs,
        cgram: &[u16; 256],
        binder: &Binder<'_>,
    ) {
        let pixel =
            self.lookup.pixel(probe, regs, cgram, binder, &mut self.memo, &mut self.portrait);
        self.pixels[(line - self.first_line) * self.width + x] = pixel;
    }
}

/// The memo of [`Lookup::pixel`]: the last character asked about, and the answer.
pub(crate) type Memo = Option<((bool, u16, u8, u32), Choice)>;

/// What the capture of pixels reads of the pack: which pictures cover which characters.
pub(crate) struct Lookup {
    pack: Arc<Pack>,
    /// Whether the pixels record what they show for the tally.
    track: bool,
    /// For each asset of the pack's index, the pictures that cover it.
    asset_pictures: Vec<Vec<u32>>,
    /// The assets that are portraits or faces (they hold pictures somewhere).
    portrait_asset: Vec<bool>,
    /// The pack's background layers by class, and the ones the frame being captured shows.
    bg_classes: bgart::Classes,
    bg_slots: Vec<Slot>,
}

impl Lookup {
    /// What the art passes need of one resolved pixel. `portrait` counts the characters of
    /// portraits that have no picture.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pixel(
        &self,
        probe: &Probe<'_>,
        regs: &LineRegs,
        cgram: &[u16; 256],
        binder: &Binder<'_>,
        memo: &mut Memo,
        portrait: &mut u64,
    ) -> ArtPixel {
        let rendered = probe.rendered;
        let object = rendered.layer == Layer::Obj;
        let mut pixel = ArtPixel {
            oam: if object { rendered.oam } else { crate::NO_OBJECT },
            prio: rendered.priority,
            mode_01: regs.bg_mode < 2,
            brightness: regs.brightness,
            ..ArtPixel::NONE
        };
        if regs.bg_mode < 2 || regs.bg_mode == 7 {
            pixel.bg = bgart::record(&self.bg_slots, probe, regs);
        }
        if object {
            pixel.plan = probe.plan();
        }
        let bg = match rendered.layer {
            Layer::Bg1 => Some(0),
            Layer::Bg2 => Some(1),
            Layer::Bg3 => Some(2),
            Layer::Bg4 => Some(3),
            _ => None,
        };
        let four_bit = match bg {
            Some(bg) => regs.bg_mode == 1 && bg < 2,
            None => object,
        };
        if four_bit && bg.is_some() {
            pixel.bg4 = true;
            pixel.bg_char = rendered.char;
            pixel.bg_pos = rendered.pos;
            pixel.bg_palette = rendered.palette;
        }
        if self.track {
            pixel.shown = match rendered.layer {
                Layer::Backdrop => Shown::Nothing,
                Layer::Obj => Shown::Obj,
                _ if regs.bg_mode == 7 => Shown::Plane,
                _ => Shown::Bg,
            };
            pixel.char = rendered.char;
            if matches!(pixel.shown, Shown::Bg | Shown::Obj) {
                pixel.bpp = if four_bit { 4 } else { 2 };
                if let Some(tile) = binder.tile(pixel.bpp, rendered.char) {
                    pixel.asset = tile.asset;
                    pixel.tile = tile.tile;
                }
            }
        }
        if !four_bit || self.pack.art.pictures().is_empty() {
            return pixel;
        }
        // The eight pixels of a character in a row share the answer, unless the palette decides
        let key = (object, rendered.char, rendered.palette, binder.epoch());
        let choice = match *memo {
            Some((last, choice)) if last == key => choice,
            _ => {
                let (choice, from_palette) =
                    self.choose(binder, cgram, object, rendered.char, rendered.palette);
                *memo = (!from_palette).then_some((key, choice));
                choice
            }
        };
        match choice {
            Choice::None => {}
            Choice::Portrait => *portrait += 1,
            Choice::Picture(at, cell) => {
                pixel.picture = at;
                pixel.row = rendered.palette + if object { 8 } else { 0 };
                pixel.cell = cell;
                pixel.pos = rendered.pos;
                if !object {
                    pixel.plan = probe.plan();
                }
            }
        }
        pixel
    }

    /// The picture a four-bit character shows a cell of, if the pack has one; and whether the
    /// character's palette decided between several.
    fn choose(
        &self,
        binder: &Binder<'_>,
        cgram: &[u16; 256],
        object: bool,
        char: u16,
        palette: u8,
    ) -> (Choice, bool) {
        let Some(tile) = binder.tile(4, char) else { return (Choice::None, false) };
        let candidates = &self.asset_pictures[tile.asset as usize];
        if candidates.is_empty() {
            let portrait = self.portrait_asset[tile.asset as usize];
            return (if portrait { Choice::Portrait } else { Choice::None }, false);
        }
        let mut covering: Vec<(u32, u8)> = Vec::new();
        for &at in candidates {
            let tiles = &self.pack.art.pictures()[at as usize].tiles;
            if let Some(cell) = tiles.iter().position(|&t| u32::from(t) == tile.tile) {
                covering.push((at, cell as u8));
            }
        }
        let (choice, from_palette) = match covering.as_slice() {
            [] => (None, false),
            [only] => (Some(*only), false),
            many => {
                let base = if object { 0x80 } else { 0 } + usize::from(palette) * 16;
                let mut row = [[0_u8; 3]; 16];
                for (index, color) in row.iter_mut().enumerate() {
                    *color = poses::rgb555(cgram[base + index]);
                }
                let best = many
                    .iter()
                    .min_by_key(|&&(at, _)| {
                        let palette = &self.pack.art.pictures()[at as usize].palette;
                        (palette.distance(&row), u8::from(palette.mask == 0))
                    })
                    .copied();
                (best, true)
            }
        };
        match choice {
            Some((at, cell)) => (Choice::Picture(at, cell), from_palette),
            None if self.portrait_asset[tile.asset as usize] => (Choice::Portrait, from_palette),
            None => (Choice::None, from_palette),
        }
    }
}

/// Counts a pixel that a sprite of a pose of the kind shows, by what became of the pose: `mask`
/// holds the sprites (a bit for each OAM entry) of the poses with art (0), the missing (1) and the
/// partial ones (2), all (3) and the recoloured (4).
fn count_pixel(counts: &mut KindPixels, mask: &[u128; 5], bit: u128, pixel: &ArtPixel) {
    counts.total += 1;
    if mask[0] & bit != 0 {
        counts.art += 1;
        counts.recoloured += u64::from(mask[4] & bit != 0);
    } else if pixel.picture != NO_PICTURE {
        // A face or portrait drawn as sprites: the picture's art covers them
        counts.art += 1;
    } else if mask[1] & bit != 0 {
        counts.missing += 1;
    } else {
        counts.partial += 1;
    }
}

/// The record of a frame set aside while the picture under its poses is drawn (see [`ArtState::begin_under`]).
pub(crate) struct UnderCapture {
    first: Vec<ArtPixel>,
    stats: ArtStats,
}

/// Gives each pixel of `pixels` (the frame's record) that one of the `hidden` sprites won, in rows `rows`, the
/// backgrounds' record of the same pixel in `under` (the record of the frame drawn without those sprites): the art of
/// the backgrounds is drawn where the pose's art leaves the pixel clear only as the layers are there with the sprite
/// gone. The record of the sprite itself can only guess at them: it takes every layer the line shows, under a window
/// that hides one too, and none in front of them that has no art (an explosion's ground), where the picture under the
/// poses shows what is really there. The rest of the pixel (its object, priority, colour math) stays the sprite's.
pub(crate) fn merge_under(
    pixels: &mut [ArtPixel],
    under: &[ArtPixel],
    width: usize,
    hidden: u128,
    rows: Option<(usize, usize)>,
) {
    let Some((first, past)) = rows else { return };
    let end = (past * width).min(pixels.len()).min(under.len());
    let start = (first * width).min(end);
    for (pixel, below) in pixels[start..end].iter_mut().zip(&under[start..end]) {
        if pixel.oam != crate::NO_OBJECT && hidden >> (pixel.oam & 127) & 1 != 0 {
            pixel.bg = below.bg;
        }
    }
}

/// `log` with BG1 off on every line: the frame under a Mode 7 plane (see [`ArtState::plane_replaces`]).
pub(crate) fn without_bg1(log: &FrameLog) -> FrameLog {
    let mut under = log.clone();
    for entry in &mut under.entries {
        entry.regs.main_bg[0] = false;
    }
    under
}

/// The frame rows the picture under the matched poses is needed for: their boxes and the margin
/// the art grows into, as `(first, past the last)`; `None` when there is no pose.
pub(crate) fn pose_rows(found: &[(Pose, u32)], height: usize) -> Option<(usize, usize)> {
    let mut rows: Option<(i32, i32)> = None;
    for (pose, _) in found {
        let first = pose.org.1 + poses::ROW_OF_CAPTURE;
        let span = (first - draw::GROWTH - 1, first + pose.h as i32 + draw::GROWTH + 1);
        rows = Some(rows.map_or(span, |(a, b)| (a.min(span.0), b.max(span.1))));
    }
    rows.map(|(a, b)| (a.clamp(0, height as i32) as usize, b.clamp(0, height as i32) as usize))
}

/// The share of a pose's opaque pixels with no opaque pixel to their left or right, per mille.
fn stripy_permille(pose: &Pose) -> u32 {
    let mut total = 0_u32;
    let mut alone = 0_u32;
    for y in 0..pose.h {
        for x in 0..pose.w {
            if pose.arr[y * pose.w + x] == 0 {
                continue;
            }
            total += 1;
            let left = x > 0 && pose.arr[y * pose.w + x - 1] != 0;
            let right = x + 1 < pose.w && pose.arr[y * pose.w + x + 1] != 0;
            if !left && !right {
                alone += 1;
            }
        }
    }
    (alone * 1000).checked_div(total).unwrap_or(0)
}

/// The resolver priority of an object with OAM priority bits `bits` (lower is in front).
pub(crate) fn object_priority(mode_01: bool, bits: u8) -> u8 {
    match (mode_01, bits) {
        (_, 3) => 0,
        (true, 2) => 3,
        (true, 1) => 6,
        (true, _) => 9,
        (false, 2) => 2,
        (false, 1) => 4,
        (false, _) => 6,
    }
}

pub use draw::{COLOUR_CLASSES, Drawn, colour_class};
pub(crate) use draw::{Placed, draw_objects, draw_pictures, draw_shadows, draw_variant};
