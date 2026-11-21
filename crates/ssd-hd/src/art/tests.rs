//! The art passes on synthetic frames: a pack of a few pictures made in a temporary folder, and
//! frames with one tile or one sprite.

use super::poses::{self, Pose};
use super::{ArtSource, ArtState, Conditions};
use crate::art_index::{ArtIndex, Object, Palette, Picture};
use crate::pack::{Asset, Index, Manifest, Mode, Pack, PoseVariant, word_key};
use crate::world::{HudPlacement, Rom, WideOptions, WideRenderer};
use crate::{Fate, HdOptions, ItemKey, Look, Renderer, SpriteLine, TallyLayer};
use ssd_oracle::{FrameLog, LineEntry, LineRegs};
use std::sync::Arc;

pub(super) fn regs() -> LineRegs {
    LineRegs {
        brightness: 15,
        bg_mode: 1,
        main_bg: [true, false, false, false],
        main_obj: true,
        bg_map_base: [0x0400, 0, 0, 0],
        bg_char_base: [0x1000, 0, 0, 0],
        ..LineRegs::default()
    }
}

/// A frame of 224 lines with the object sprites evaluated and rebuilt on every line.
pub(super) fn log(regs: LineRegs) -> FrameLog {
    let entries = (1..=224_u16)
        .map(|scanline| LineEntry {
            scanline,
            from_pixel: 0,
            regs,
            sprites: SpriteLine { eval_line: scanline - 1, rebuilt: true, ..SpriteLine::default() },
        })
        .collect();
    let mut log = FrameLog {
        complete: true,
        height: 224,
        h_hi_res: false,
        v_hi_res: false,
        entries,
        cgram_start: vec![0; 256],
        cgram_writes: Vec::new(),
        vram: vec![0; 32768],
        vram_writes: Vec::new(),
        oam_low: vec![0; 512],
        oam_high: vec![0; 32],
        oam_writes: Vec::new(),
    };
    for n in 0..128 {
        log.oam_low[n * 4 + 1] = 0xF0;
    }
    log
}

/// A solid 4x PNG of `width` x `height` art pixels in `rgba`, with a different colour in the
/// cell of each 32 x 32 block when `cells` (red grows with the cell number).
pub(super) fn write_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: impl Fn(u32, u32) -> [u8; 4],
) {
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&rgba(x, y));
        }
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::png::write_rgba(path, width, height, &data).unwrap();
}

pub(super) fn pack_of(dir: &std::path::Path, assets: Vec<Asset>, art: ArtIndex) -> Arc<Pack> {
    let index = Index::new(assets);
    let manifest = Manifest {
        format: crate::pack::FORMAT,
        scale: 4,
        converter: 1,
        rom_crc32: "00000000".to_owned(),
        assets: index.assets().len(),
    };
    Arc::new(Pack {
        dir: dir.to_path_buf(),
        manifest,
        index,
        art,
        maps: crate::mapart::Index::default(),
        bg: crate::bgart::Index::default(),
        mode7: crate::mode7art::Index::default(),
        variants: Vec::new(),
        pose_art: Vec::new(),
    })
}

/// A 4-bit character whose row 0 is colour 1 in every column, the other rows empty.
fn character() -> [u16; 16] {
    let mut chars = [0_u16; 16];
    chars[0] = 0x00FF;
    chars
}

pub(super) fn options(scale: u32) -> HdOptions {
    HdOptions { scale, look: Look::Crisp }
}

pub(super) fn pixel(frame: &crate::HdFrame, x: usize, y: usize) -> [u8; 3] {
    let at = (y * frame.width as usize + x) * 4;
    [frame.rgba[at], frame.rgba[at + 1], frame.rgba[at + 2]]
}

#[test]
fn a_bound_character_draws_its_cell_of_the_picture_flips_included() {
    let dir = tempfile::tempdir().unwrap();
    // The picture: cell (cx, cy) is the colour (cx * 60, cy * 60, 7), and within the cell the
    // art's left half is darker, so a flip shows
    write_png(&dir.path().join("art/p.png"), 128, 128, |x, y| {
        let (cx, cy) = (x / 32, y / 32);
        let dark = if x % 32 < 16 { 0 } else { 40 };
        [(cx * 60 + dark) as u8, (cy * 60) as u8, 7, 255]
    });
    // The asset has the character at its tile 5, which is cell 5 (x 1, y 1) of the picture
    let mut keys = vec![0_u64; 16];
    keys[5] = word_key(&character());
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys,
        art: "base/x.png".to_owned(),
    };
    let art = ArtIndex::new(
        vec![Picture {
            name: "obj-00".to_owned(),
            asset: asset.id.clone(),
            tiles: std::array::from_fn(|n| n as u16),
            palette: Palette::default(),
            art: "art/p.png".to_owned(),
        }],
        Vec::new(),
    );
    let pack = pack_of(dir.path(), vec![asset], art);

    let mut log = log(regs());
    log.vram[0x1000 + 16..0x1000 + 32].copy_from_slice(&character()); // character 1 of the base
    // BG1 shows character 1 at map row 1 (a BG shows one row below its scanline: the frame row 7
    // is the first row of that tile) and again, flipped across, next to it
    log.vram[0x0400 + 32] = 1;
    log.vram[0x0400 + 33] = 1 | 0x4000;
    log.cgram_start[1] = 0x001F;

    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&pack)));
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    // The first tile's row 1 (the first frame row is scanline 1, a BG shows its row 1 there)...
    // The character's row 0 is the only one with colour; the frame's row 0 shows row 1: empty.
    // Row 1 of the frame is row 2 of the character: also empty; so look at what is drawn from the
    // art instead: the tile's pixels are transparent and draw nothing, and only the colour-1 row
    // is a pixel of the character: frame row 7 shows the tile's row 0 (rows are one down).
    // The pixel's block at x 0..4 is art cell (1, 1), row 0 of the character, columns 0..
    let at_row_0 = 7 * 4; // the HD row of the frame row 7
    let left = pixel(&picture, 0, at_row_0);
    let right = pixel(&picture, 31, at_row_0);
    assert_eq!(left[1], 60, "cell (1, 1) of the picture: its green is 60");
    assert_eq!(left[0], 60, "dark half: red 60");
    assert_eq!(right[0], 100, "the lighter half of the cell is on its right");
    // The flipped tile next to it shows the cell mirrored: its left is the cell's lighter half
    let flipped_left = pixel(&picture, 32, at_row_0);
    assert_eq!(flipped_left[0], 100);
    assert_eq!(pixel(&picture, 32 + 31, at_row_0)[0], 60);
    let stats = renderer.art_stats();
    assert_eq!(stats.picture_pixels, 16, "two tiles of eight pixels");
}

#[test]
fn a_picture_chosen_by_palette_when_two_cover_the_same_characters() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/a.png"), 128, 128, |_, _| [10, 0, 0, 255]);
    write_png(&dir.path().join("art/b.png"), 128, 128, |_, _| [0, 10, 0, 255]);
    let mut keys = vec![0_u64; 16];
    keys[0] = word_key(&character());
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys,
        art: String::new(),
    };
    let mut palette_a = Palette { mask: 0b10, ..Palette::default() };
    palette_a.colors[1] = [255, 0, 0];
    let mut palette_b = palette_a;
    palette_b.colors[1] = [0, 0, 255];
    let picture = |name: &str, palette, art: &str| Picture {
        name: name.to_owned(),
        asset: asset.id.clone(),
        tiles: std::array::from_fn(|n| n as u16),
        palette,
        art: art.to_owned(),
    };
    let art = ArtIndex::new(
        vec![picture("a", palette_a, "art/a.png"), picture("b", palette_b, "art/b.png")],
        Vec::new(),
    );
    let pack = pack_of(dir.path(), vec![asset], art);
    let mut log = log(regs());
    log.vram[0x1000 + 16..0x1000 + 32].copy_from_slice(&character());
    log.vram[0x0400 + 32] = 1;
    // BG1 palette 0 colour 1 is blue: the second picture
    log.cgram_start[1] = 0x7C00;
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 0, 28), [0, 10, 0]);
}

/// A fighter of one 16 x 16 sprite with palette 1, solid colour 2 (green), at `(x, y)`.
pub(super) fn fighter_log(x: u8, y: u8) -> (FrameLog, Pose) {
    let mut log = log(regs());
    // Sprite 0, tile 0, palette 1, the large size; the object characters are at word 0 of VRAM
    log.oam_low[0] = x;
    log.oam_low[1] = y;
    log.oam_low[2] = 0;
    log.oam_low[3] = 1 << 1;
    log.oam_high[0] = 0b10;
    // Colour 2 is plane 1: the high byte of the first eight words of a character; the four
    // characters of a 16 x 16 sprite are tiles 0, 1, 16 and 17
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            log.vram[tile * 16 + row] = 0xFF00;
        }
    }
    log.cgram_start[0x80 + 16 + 2] = 0x03E0; // palette 1, colour 2: green
    let sprites = poses::sprites(&log);
    // (a field group is found in every frame too: it is not what these helpers are for)
    let poses: Vec<Pose> =
        poses::poses(&log, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(poses.len(), 1);
    (log, poses.into_iter().next().unwrap())
}

pub(super) fn object(pose: &Pose, art: &str, luma: bool) -> Object {
    Object {
        key: pose.key,
        id: "battle-x-000".to_owned(),
        luma,
        palette: Palette::default(),
        mask: Vec::new(),
        width: pose.w as u16,
        height: pose.h as u16,
        art: art.to_owned(),
    }
}

#[test]
fn a_pose_is_found_by_its_key_and_its_art_replaces_the_sprites_pixels() {
    let dir = tempfile::tempdir().unwrap();
    // 16 x 16 source pixels, 64 x 64 art pixels: a solid blue square
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (frame, pose) = fighter_log(40, 50);
    assert_eq!((pose.w, pose.h), (16, 16));
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/o.png", false)]),
    );
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();

    // The sprite's row 0 is the frame row 50, its column 0 the column 40
    assert_eq!(pixel(&plain, 41 * 4 + 1, 51 * 4 + 1), [0, 0xFF, 0]);
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255]);
    assert_eq!(pixel(&drawn, 40 * 4, 50 * 4), [0, 0, 255], "the first pixel of the pose");
    assert_eq!(pixel(&drawn, 55 * 4 + 3, 65 * 4 + 3), [0, 0, 255], "and the last");
    assert_ne!(pixel(&drawn, 40 * 4 - 1, 50 * 4), [0, 0, 255], "nothing past its box");
    assert_ne!(pixel(&drawn, 40 * 4, 50 * 4 - 1), [0, 0, 255]);
    // Away from the pose nothing changes
    assert_eq!(pixel(&drawn, 5, 5), pixel(&plain, 5, 5));
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.total, stats.fighters.art), (256, 256));
    assert_eq!(stats.poses.iter().filter(|pose| !pose.field).count(), 1);
    assert!(stats.poses[0].matched);
}

#[test]
fn a_pose_the_index_lacks_stays_the_look() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let mut other = object(&pose, "art/none.png", false);
    other.key ^= 1;
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(2)).unwrap();
    renderer.set_art(Some(pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![other]))));
    let drawn = renderer.render_hd(&frame, options(2)).unwrap();
    assert_eq!(drawn, plain);
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.total, stats.fighters.art), (256, 0));
    assert_eq!(stats.fighters.missing, 256, "a whole pose the capture would have kept");
}

#[test]
fn a_pose_whose_art_file_is_missing_stays_as_the_game_draws_it() {
    let dir = tempfile::tempdir().unwrap();
    // The index names art that is not there (an import in progress, a deleted file): the sprites
    // are not hidden for art that cannot be drawn
    let (frame, pose) = fighter_log(40, 50);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/gone.png", false)]),
    );
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(drawn, plain, "the sprite is still there");
    assert!(renderer.art_stats().poses.iter().all(|pose| !pose.matched));
    // The same through the wide view
    let mut wide = WideRenderer::new();
    wide.set_art(Some(Arc::clone(&renderer_pack(&dir, &pose))));
    let options = WideOptions { width: 372, hud: HudPlacement::Window };
    let drawn =
        wide.render_hd(Rom(&[]), 0x02, None, &frame, options, super::tests::options(4)).unwrap();
    assert_eq!(pixel(&drawn.picture, (58 + 41) * 4 + 1, 51 * 4 + 1), [0, 0xFF, 0]);
}

fn renderer_pack(dir: &tempfile::TempDir, pose: &Pose) -> Arc<Pack> {
    pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(pose, "art/gone.png", false)]),
    )
}

#[test]
fn a_pose_behind_a_background_tile_is_drawn_only_where_the_original_showed_it() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (mut frame, pose) = fighter_log(40, 50);
    // BG1 character 1 (solid colour 1) with the priority bit at map tile (5, 6): it covers the
    // columns 40..48 and the frame rows 47..55 (a BG shows one row below its scanline), in front
    // of the sprite's priority 0
    let mut solid = [0_u16; 16];
    solid[..8].fill(0x00FF);
    frame.vram[0x1000 + 16..0x1000 + 32].copy_from_slice(&solid);
    frame.vram[0x0400 + 6 * 32 + 5] = 1 | 0x2000;
    frame.cgram_start[1] = 0x7FFF;
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/o.png", false)]),
    );
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_ne!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "behind the tile");
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 60 * 4 + 1), [0, 0, 255], "below it");
    assert_eq!(pixel(&drawn, 50 * 4 + 1, 52 * 4 + 1), [0, 0, 255], "beside it");
    let stats = renderer.art_stats();
    assert_eq!(stats.fighters.total, 256 - 5 * 8, "the covered part is not the sprite's pixels");
}

#[test]
fn luma_art_takes_its_colours_from_the_original() {
    let dir = tempfile::tempdir().unwrap();
    // Greyscale art, bright on the left half and half as bright on the right
    write_png(&dir.path().join("art/o.png"), 64, 64, |x, _| {
        let level = if x < 32 { 200 } else { 100 };
        [level, level, level, 255]
    });
    let (frame, pose) = fighter_log(40, 50);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/o.png", true)]),
    );
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // The art's reference luminance keeps 97 percent of the original's colour on average (here
    // 106): the left is the original's full green, the right 100 / 106 of it
    assert_eq!(pixel(&drawn, 41 * 4, 51 * 4), [0, 0xFF, 0]);
    assert_eq!(pixel(&drawn, 50 * 4, 51 * 4), [0, 240, 0]);
}

#[test]
fn shading_follows_the_colour_math_and_the_brightness() {
    use crate::screen::{Math, Plan};
    let plain = Plan { force_black: false, math: None };
    assert_eq!(super::draw::shade(plain, 15, [10, 20, 30]), [10, 20, 30]);
    assert_eq!(super::draw::shade(plain, 0, [10, 20, 30]), [0, 0, 0]);
    // Half brightness rounds
    assert_eq!(super::draw::shade(plain, 8, [255, 100, 1]), [136, 53, 1]);
    // Subtracting the fixed colour: the 5-bit red channel 10 is 82 of 255
    let math =
        Plan { force_black: false, math: Some(Math { sub: 10, subtract: true, divide: false }) };
    assert_eq!(super::draw::shade(math, 15, [200, 50, 50]), [118, 50, 50]);
    let black = Plan { force_black: true, math: None };
    assert_eq!(super::draw::shade(black, 15, [200, 50, 50]), [0, 0, 0]);
}

#[test]
fn the_wide_view_draws_the_art_beside_the_black_borders_of_a_picture() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (frame, pose) = fighter_log(40, 50);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/o.png", false)]),
    );
    let mut wide = WideRenderer::new();
    wide.set_art(Some(pack));
    let options = WideOptions { width: 372, hud: HudPlacement::Window };
    // The game mode of a battle: a picture of the original's 256 pixels, black on both sides
    let drawn =
        wide.render_hd(Rom(&[]), 0x02, None, &frame, options, super::tests::options(4)).unwrap();
    assert_eq!((drawn.picture.width, drawn.picture.height), (372 * 4, 224 * 4));
    // The window starts 58 pixels in: the pose is 58 pixels further right than in the 256-wide frame
    assert_eq!(drawn.logical.x, 58);
    assert_eq!(drawn.picture.logical.x, 58 * 4);
    assert_eq!(pixel(&drawn.picture, (58 + 41) * 4 + 1, 51 * 4 + 1), [0, 0, 255]);
    assert_eq!(pixel(&drawn.picture, 10, 10), [0, 0, 0], "black beside the window");
    let stats = wide.art_stats();
    assert_eq!((stats.fighters.total, stats.fighters.art), (256, 256));
    // Without art the same frame is the look alone, and the sides are the same
    wide.set_art(None);
    let plain =
        wide.render_hd(Rom(&[]), 0x02, None, &frame, options, super::tests::options(4)).unwrap();
    assert_eq!(pixel(&plain.picture, (58 + 41) * 4 + 1, 51 * 4 + 1), [0, 0xFF, 0]);
    assert_eq!(pixel(&plain.picture, 10, 10), [0, 0, 0]);
}

#[test]
fn a_fighter_fading_in_as_stripes_is_drawn_from_the_whole_pose_where_it_shows() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (full, whole) = fighter_log(40, 50);
    // The same sprite with every other column left out: its pixels are not the pose's key
    let (mut striped, _) = fighter_log(40, 50);
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            striped.vram[tile * 16 + row] = 0xAA00;
        }
    }
    let mut palette = Palette { mask: 0b100, ..Palette::default() };
    palette.colors[2] = [0, 255, 0];
    let mut held = object(&whole, "art/o.png", false);
    held.palette = palette;
    held.mask = vec![0xFF; 32];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![held]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));

    let drawn = renderer.render_hd(&striped, options(4)).unwrap();
    // Columns 0, 2, 4 of the sprite show; the art is drawn on them and not between
    assert_eq!(pixel(&drawn, 40 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
    assert_eq!(pixel(&drawn, 42 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
    assert_ne!(
        pixel(&drawn, 41 * 4 + 1, 55 * 4 + 1),
        [0, 0, 255],
        "a column the original left out"
    );
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.total, stats.fighters.art), (128, 128));

    // The whole pose is found by its key as before
    let drawn = renderer.render_hd(&full, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
}

#[test]
fn a_whole_fighter_with_no_art_is_not_taken_for_a_piece_of_another_pose() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (full, whole) = fighter_log(40, 50);
    // The index holds a pose of the same silhouette (and colours) under another key: a pose the
    // original draws whole (no stripes, not at an edge) is its own, never a piece of it
    let mut other = object(&whole, "art/o.png", false);
    other.key ^= 1;
    other.mask = vec![0xFF; 32];
    other.palette = Palette { mask: 0b100, ..Palette::default() };
    other.palette.colors[2] = [0, 255, 0];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![other]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&full, options(4)).unwrap();
    assert_ne!(pixel(&drawn, 41 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
    assert!(!renderer.art_stats().poses.iter().any(|p| p.matched));
}

#[test]
fn every_scale_draws_the_pose_and_the_picture_in_the_same_place() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/p.png"), 128, 128, |_, _| [255, 0, 0, 255]);
    let (mut frame, pose) = fighter_log(100, 100);
    // A BG1 character with colour at map row 1 (frame row 7): the picture's cell
    frame.vram[0x1000 + 16..0x1000 + 32].copy_from_slice(&character());
    frame.vram[0x0400 + 32] = 1;
    frame.cgram_start[1] = 0x001F;
    let mut keys = vec![0_u64; 16];
    keys[0] = word_key(&character());
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys,
        art: String::new(),
    };
    let picture = Picture {
        name: "obj-00".to_owned(),
        asset: asset.id.clone(),
        tiles: std::array::from_fn(|n| n as u16),
        palette: Palette::default(),
        art: "art/p.png".to_owned(),
    };
    let art = ArtIndex::new(vec![picture], vec![object(&pose, "art/o.png", false)]);
    let pack = pack_of(dir.path(), vec![asset], art);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    for scale in 1..=8_u32 {
        let drawn = renderer.render_hd(&frame, options(scale)).unwrap();
        let s = scale as usize;
        assert_eq!((drawn.width, drawn.height), (256 * scale, 224 * scale));
        assert_eq!(
            pixel(&drawn, 100 * s, 100 * s),
            [0, 0, 255],
            "the pose's first pixel at {scale}x"
        );
        assert_eq!(pixel(&drawn, 115 * s + s - 1, 115 * s + s - 1), [0, 0, 255], "and its last");
        assert_eq!(pixel(&drawn, 0, 7 * s), [255, 0, 0], "the picture at {scale}x");
    }
}

/// A ground shadow: one 16 x 16 sprite of palette 3 (an effect's) in one dark grey, at `(x, y)`,
/// an ellipse of four rows (the rows 6 to 9 of the sprite, 10 and 16 pixels wide) made of the
/// characters 2, 3, 18 and 19. Character 0 is a solid block (a fighter's).
fn shadow_log(x: u8, y: u8) -> FrameLog {
    let mut log = log(regs());
    log.oam_low[0] = x;
    log.oam_low[1] = y;
    log.oam_low[2] = 2;
    log.oam_low[3] = 3 << 1;
    log.oam_high[0] = 0b10;
    for row in 0..8 {
        log.vram[row] = 0xFF00;
    }
    // Colour 2 is plane 1, the high byte of a row's word
    let rows = [(2_usize, 6, 0x1F), (3, 6, 0xF8), (2, 7, 0xFF), (3, 7, 0xFF)];
    for (tile, row, bits) in rows {
        log.vram[tile * 16 + row] = bits << 8;
    }
    let rows = [(18_usize, 0, 0xFF), (19, 0, 0xFF), (18, 1, 0x1F), (19, 1, 0xF8)];
    for (tile, row, bits) in rows {
        log.vram[tile * 16 + row] = bits << 8;
    }
    log.cgram_start[0x80 + 3 * 16 + 2] = 0x318C; // 12, 12, 12: grey of about 99
    log
}

#[test]
fn a_dark_flat_effect_is_a_shadow_drawn_from_the_originals_colour_with_a_soft_edge() {
    let dir = tempfile::tempdir().unwrap();
    // The index has art for something else: a shadow needs none, and the art that was made of a
    // shadow's pixels (a bright flash) is not used
    let frame = shadow_log(40, 50);
    let sprites = poses::sprites(&frame);
    let found: Vec<Pose> =
        poses::poses(&frame, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(found.len(), 1);
    assert!(found[0].shadow && !found[0].fighter);
    write_png(&dir.path().join("art/flash.png"), 64, 64, |_, _| [255, 255, 0, 255]);
    let flash = object(&found[0], "art/flash.png", false);
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![flash]));
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();

    // The ellipse's rows are the frame rows 57 to 60
    let grey = pixel(&plain, 48 * 4, 58 * 4);
    assert!(grey[0] > 90 && grey[0] < 110 && grey[0] == grey[1] && grey[1] == grey[2], "{grey:?}");
    assert_eq!(pixel(&drawn, 48 * 4, 58 * 4), grey, "inside: the original's colour, not the flash");
    assert_eq!(pixel(&drawn, 44 * 4 + 1, 58 * 4 + 1), grey);
    // The edge: a pixel just outside the sprite is partly shadow, a pixel further out is the picture
    let edge = pixel(&drawn, 40 * 4 + 1, 58 * 4 + 1);
    assert!(edge[0] > 0 && edge[0] < grey[0], "soft edge {edge:?}");
    assert_eq!(pixel(&drawn, 40 * 4 - 8, 58 * 4), pixel(&plain, 40 * 4 - 8, 58 * 4));
    let stats = renderer.art_stats();
    assert_eq!((stats.effects.total, stats.effects.art), (52, 52));
}

#[test]
fn a_shadow_is_a_smooth_ellipse_that_darkens_a_lighter_picture() {
    let dir = tempfile::tempdir().unwrap();
    // The same shadow over a bright backdrop (colour 0, 31 of 31 on every channel): the shadow's
    // grey of about 99 is the darkness the original had against 255
    let mut frame = shadow_log(40, 50);
    frame.cgram_start[0] = 0x7FFF;
    let sprites = poses::sprites(&frame);
    let found: Vec<Pose> =
        poses::poses(&frame, &sprites).into_iter().filter(|pose| !pose.field).collect();
    write_png(&dir.path().join("art/flash.png"), 64, 64, |_, _| [255, 255, 0, 255]);
    let flash = object(&found[0], "art/flash.png", false);
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![flash]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert!(found[0].shadow);
    let middle = pixel(&drawn, 48 * 4, 58 * 4);
    assert!(middle[0] > 90 && middle[0] < 110, "multiplied by the original's darkness {middle:?}");
    // Across the top of the ellipse (rows 56 to 59, 16 wide in the middle): the shade goes from
    // the picture's white to the shadow's grey in more than the four steps of the original's
    // rows, and never gets lighter again going down
    let column: Vec<u8> = (55 * 4..58 * 4).map(|y| pixel(&drawn, 48 * 4, y)[0]).collect();
    assert_eq!(column[0], 255, "{column:?}");
    assert!(column.windows(2).all(|pair| pair[0] >= pair[1]), "{column:?}");
    let levels: std::collections::BTreeSet<u8> = column.iter().copied().collect();
    assert!(levels.len() >= 5, "{column:?}");
    // The corner of the box, which the stepped ellipse leaves out too, is the picture's
    assert_eq!(pixel(&drawn, 40 * 4, 56 * 4), [255, 255, 255]);
    assert_eq!(pixel(&drawn, 40 * 4 - 8, 58 * 4), [255, 255, 255]);
}

#[test]
fn a_shadow_goes_under_a_fighter_drawn_from_art() {
    let dir = tempfile::tempdir().unwrap();
    // A fighter (palette 1) over the shadow: the fighter's art covers part of it, and the
    // shadow's silhouette continues beneath
    write_png(&dir.path().join("art/o.png"), 64, 64, |x, _| {
        if x < 32 { [0, 0, 255, 255] } else { [0, 0, 0, 0] }
    });
    // Sprite 0 is the fighter, in front, and sprite 1 the shadow, eight pixels to its right
    let mut frame = shadow_log(48, 50);
    frame.oam_low.copy_within(0..4, 4);
    frame.oam_low[0..4].copy_from_slice(&[40, 50, 0, 1 << 1]);
    frame.oam_high[0] |= 0b10 << 2;
    frame.cgram_start[0x80 + 16 + 2] = 0x03E0;
    let sprites = poses::sprites(&frame);
    let found = poses::poses(&frame, &sprites);
    let fighter = found.iter().find(|pose| pose.fighter).expect("a fighter");
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(fighter, "art/o.png", false)]),
    );
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // Where the fighter's art is transparent (its right half) the shadow's grey shows, though
    // the original's fighter sprite covered it there
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "the fighter's art");
    let under = pixel(&drawn, 52 * 4, 58 * 4);
    assert!(under[0] == under[1] && under[0] > 90 && under[0] < 110, "{under:?}");
}

/// A picture of the portrait asset with one character, made for colour 1 = red (art solid red).
pub(super) fn red_picture_pack(dir: &std::path::Path, art_colour: [u8; 4]) -> Arc<Pack> {
    write_png(&dir.join("art/a.png"), 128, 128, |_, _| art_colour);
    let mut keys = vec![0_u64; 16];
    keys[0] = word_key(&character());
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys,
        art: String::new(),
    };
    let mut palette = Palette { mask: 0b10, ..Palette::default() };
    palette.colors[1] = [255, 0, 0];
    let picture = Picture {
        name: "a".to_owned(),
        asset: asset.id.clone(),
        tiles: std::array::from_fn(|n| n as u16),
        palette,
        art: "art/a.png".to_owned(),
    };
    pack_of(dir, vec![asset], ArtIndex::new(vec![picture], Vec::new()))
}

pub(super) fn portrait_log(colour_1: u16) -> FrameLog {
    let mut log = log(regs());
    log.vram[0x1000 + 16..0x1000 + 32].copy_from_slice(&character());
    log.vram[0x0400 + 32] = 1;
    log.cgram_start[1] = colour_1;
    log
}

#[test]
fn a_picture_with_one_art_is_drawn_in_the_palette_row_the_game_uses() {
    let dir = tempfile::tempdir().unwrap();
    let pack = red_picture_pack(dir.path(), [200, 20, 20, 255]);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    // The palette the art was made for (colour 1 red): the art as it is
    let same = renderer.render_hd(&portrait_log(0x001F), options(4)).unwrap();
    assert_eq!(pixel(&same, 0, 28), [200, 20, 20]);
    // The same tiles in a blue row: the art recoloured, still lit as the art is (not the 255 of
    // the palette's blue: the art's red was dimmer than the palette's red)
    let blue = renderer.render_hd(&portrait_log(0x7C00), options(4)).unwrap();
    let [r, g, b] = pixel(&blue, 0, 28);
    assert!(b > 150 && r < 40 && g < 40, "blue, not red: {r} {g} {b}");
    assert!(b < 255, "the art's shading is kept: {b}");
    // A frame with the palette in another place of the palette row is its own variant
    let green = renderer.render_hd(&portrait_log(0x03E0), options(4)).unwrap();
    let [r, g, b] = pixel(&green, 0, 28);
    assert!(g > 150 && r < 40 && b < 40, "green: {r} {g} {b}");
}

#[test]
fn a_pose_whose_art_was_made_for_another_palette_is_drawn_recoloured() {
    let dir = tempfile::tempdir().unwrap();
    // The art is the pose's green (palette 1, colour 2), a little darker in its lower half
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, y| {
        if y < 32 { [0, 255, 0, 255] } else { [0, 190, 0, 255] }
    });
    let (mut frame, pose) = fighter_log(40, 50);
    let mut variant = object(&pose, "art/o.png", false);
    variant.palette = Palette { mask: 0b100, ..Palette::default() };
    variant.palette.colors[2] = [0, 255, 0];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![variant]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));

    // In the palette it was made for: the art as it is, nothing recoloured
    let same = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&same, 41 * 4 + 1, 51 * 4 + 1), [0, 255, 0]);
    assert_eq!(pixel(&same, 41 * 4 + 1, 61 * 4 + 1), [0, 190, 0]);
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.art, stats.fighters.recoloured), (256, 0));
    assert!(!stats.poses[0].recoloured);

    // The same sprite in a red palette (the pose's key does not read colours): the art comes
    // out red, its lower half still the darker
    frame.cgram_start[0x80 + 16 + 2] = 0x001F;
    let red = renderer.render_hd(&frame, options(4)).unwrap();
    let (top, bottom) = (pixel(&red, 41 * 4 + 1, 51 * 4 + 1), pixel(&red, 41 * 4 + 1, 61 * 4 + 1));
    assert_eq!(top, [255, 0, 0]);
    assert!(bottom[0] > 150 && bottom[0] < 255 && bottom[1] == 0 && bottom[2] == 0, "{bottom:?}");
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.art, stats.fighters.recoloured), (256, 256));
    assert!(stats.poses[0].matched && stats.poses[0].recoloured);

    // The exact palette wins: with an art for the red palette too, that one is drawn as it is
    let mut exact = object(&pose, "art/e.png", false);
    exact.palette = Palette { mask: 0b100, ..Palette::default() };
    exact.palette.colors[2] = [255, 0, 0];
    write_png(&dir.path().join("art/e.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let mut variant = object(&pose, "art/o.png", false);
    variant.palette = Palette { mask: 0b100, ..Palette::default() };
    variant.palette.colors[2] = [0, 255, 0];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![variant, exact]));
    renderer.set_art(Some(pack));
    let both = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&both, 41 * 4 + 1, 61 * 4 + 1), [255, 0, 0]);
    assert_eq!(renderer.art_stats().fighters.recoloured, 0);
}

/// An effect-class sprite (palette 4, so keyed by its colours) of 16 x 16 pixels at `(x, y)` with
/// three colours in bands (index 1 above, 2, then 3), its top row and first pixel empty.
fn banded_log(x: u8, y: u8, colours: [u16; 3]) -> (FrameLog, Pose) {
    let mut log = log(regs());
    log.oam_low[0] = x;
    log.oam_low[1] = y;
    log.oam_low[3] = 4 << 1;
    log.oam_high[0] = 0b10;
    let band = |row: usize| match row {
        0 => 0x0000,
        1..=5 => 0x00FF,
        6..=10 => 0xFF00,
        _ => 0xFFFF,
    };
    for (tile, first_row) in [(0_usize, 0), (1, 0), (16, 8), (17, 8)] {
        for row in 0..8 {
            log.vram[tile * 16 + row] = band(first_row + row);
        }
    }
    // Not a plain block: the first pixel of the first row is empty
    log.vram[1] = 0x007F;
    for (index, colour) in colours.into_iter().enumerate() {
        log.cgram_start[0x80 + 4 * 16 + 1 + index] = colour;
    }
    let sprites = poses::sprites(&log);
    // (a field group is found in every frame too: it is not what these helpers are for)
    let poses: Vec<Pose> =
        poses::poses(&log, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(poses.len(), 1);
    (log, poses.into_iter().next().unwrap())
}

#[test]
fn a_colour_variant_of_an_effect_pose_is_found_by_its_shape_and_drawn_in_its_colours() {
    let dir = tempfile::tempdir().unwrap();
    // The art made for another palette (red, green, blue bands): its pose key is not this one's
    write_png(&dir.path().join("art/b.png"), 64, 60, |_, y| match y / 4 + 1 {
        0 => [0, 0, 0, 0],
        1..=5 => [200, 40, 40, 255],
        6..=10 => [40, 200, 40, 255],
        _ => [40, 40, 200, 255],
    });
    let (frame, pose) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    assert!(!pose.fighter);
    let mut shape = object(&pose, "art/b.png", false);
    shape.key ^= 1;
    shape.mask = vec![0; pose.arr.len().div_ceil(8)];
    for n in (0..pose.arr.len()).filter(|&n| pose.arr[n] != 0) {
        shape.mask[n / 8] |= 1 << (n % 8);
    }
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![shape]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // The first band's colour 1 (yellow: 0x03FF), the second's (cyan: 0x7FE0), the third's
    // (magenta: 0x7C1F), in the colours the sprite has, not the art's
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 53 * 4 + 1), poses::rgb555(0x03FF));
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 59 * 4 + 1), poses::rgb555(0x7FE0));
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 63 * 4 + 1), poses::rgb555(0x7C1F));
    let stats = renderer.art_stats();
    assert_eq!((stats.effects.art, stats.effects.recoloured), (239, 239));
    assert!(stats.poses[0].matched && stats.poses[0].recoloured);

    // A shape of another size is not it
    let (frame, _) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    let mut smaller = object(&pose, "art/b.png", false);
    smaller.key ^= 1;
    smaller.width = 12;
    smaller.mask = vec![0xFF; 18];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![smaller]));
    renderer.set_art(Some(pack));
    renderer.render_hd(&frame, options(4)).unwrap();
    assert!(!renderer.art_stats().poses[0].matched);
}

#[test]
fn sprites_that_show_a_picture_are_never_drawn_from_a_pose_of_the_same_shape() {
    let dir = tempfile::tempdir().unwrap();
    // The art of a pose with the silhouette of the picture's sprites (a red face-sized block):
    // found by shape only, as a colour variant of a pose would be
    write_png(&dir.path().join("art/b.png"), 64, 60, |_, _| [200, 40, 40, 255]);
    // The picture's own art, green
    write_png(&dir.path().join("art/p.png"), 128, 128, |_, _| [20, 200, 20, 255]);
    let (frame, pose) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    let mut shape = object(&pose, "art/b.png", false);
    shape.key ^= 1;
    shape.mask = vec![0; pose.arr.len().div_ceil(8)];
    for n in (0..pose.arr.len()).filter(|&n| pose.arr[n] != 0) {
        shape.mask[n / 8] |= 1 << (n % 8);
    }
    // The sprites' four characters are the first four tiles of a portrait asset's picture
    let chars: Vec<[u16; 16]> = [0_usize, 1, 16, 17]
        .iter()
        .map(|&tile| std::array::from_fn(|word| frame.vram[tile * 16 + word]))
        .collect();
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys: chars.iter().map(|c| word_key(c)).collect(),
        art: String::new(),
    };
    let picture = Picture {
        name: "face".to_owned(),
        asset: asset.id.clone(),
        tiles: std::array::from_fn(|n| n as u16),
        palette: Palette::default(),
        art: "art/p.png".to_owned(),
    };
    let with_picture =
        pack_of(dir.path(), vec![asset.clone()], ArtIndex::new(vec![picture], vec![shape.clone()]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(with_picture));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // The sprites' opaque pixels are the picture's art (green), the pose's red nowhere
    let inside = pixel(&drawn, 41 * 4 + 1, 59 * 4 + 1);
    assert_eq!(inside[1], 200, "the picture's art: {inside:?}");
    assert!(
        drawn.rgba.chunks(4).all(|p| p[..3] != [200, 40, 40]),
        "no pixel of the pose that has the same shape"
    );
    assert!(!renderer.art_stats().poses[0].matched, "the sprites are not a pose");

    // The same sprites with no picture art are the pose's, as before (the shape is a variant)
    let without = pack_of(dir.path(), vec![asset], ArtIndex::new(Vec::new(), vec![shape]));
    renderer.set_art(Some(without));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert!(renderer.art_stats().poses[0].matched);
    assert_ne!(pixel(&drawn, 41 * 4 + 1, 59 * 4 + 1)[1], 200, "the pose's art, not the picture's");
}

/// The opening's portrait with speed lines beside it: the 16 x 16 sprite of [`banded_log`] at (40, 50) whose
/// characters a portrait asset's picture covers, and left of it, touching it, an 8 x 8 sprite of the same palette
/// (tile 2, colour 1) no picture has: two lines across it at rows `lines`. The lines move from frame to frame, so the
/// sprites are one effect cluster with another key in each.
fn portrait_with_lines(lines: [usize; 2]) -> (FrameLog, Pose) {
    let (mut log, _) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    log.oam_low[4] = 32;
    log.oam_low[5] = 54;
    log.oam_low[6] = 2;
    log.oam_low[7] = 4 << 1;
    for row in lines {
        log.vram[2 * 16 + row] = 0x00FF;
    }
    let sprites = poses::sprites(&log);
    let poses: Vec<Pose> =
        poses::poses(&log, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(poses.len(), 1, "the portrait and the lines are one cluster");
    (log, poses.into_iter().next().unwrap())
}

/// The portrait asset whose picture covers the characters of the portrait sprite of [`portrait_with_lines`]
/// (tiles 0, 1, 16 and 17), its art `art`.
fn portrait_picture(frame: &FrameLog, art: &str) -> (Asset, Picture) {
    let chars: Vec<[u16; 16]> = [0_usize, 1, 16, 17]
        .iter()
        .map(|&tile| std::array::from_fn(|word| frame.vram[tile * 16 + word]))
        .collect();
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys: chars.iter().map(|c| word_key(c)).collect(),
        art: String::new(),
    };
    let picture = Picture {
        name: "face".to_owned(),
        asset: asset.id.clone(),
        tiles: std::array::from_fn(|n| n as u16),
        palette: Palette::default(),
        art: art.to_owned(),
    };
    (asset, picture)
}

/// The 4x pixels of the portrait sprite's box of [`portrait_with_lines`] (x 40 to 55, y 50 to 66).
fn portrait_box(drawn: &crate::HdFrame) -> Vec<[u8; 3]> {
    (50 * 4..67 * 4).flat_map(|y| (40 * 4..56 * 4).map(move |x| pixel(drawn, x, y))).collect()
}

#[test]
fn a_portrait_in_a_cluster_with_other_sprites_is_drawn_from_its_picture_whatever_the_art() {
    // The art made for the cluster in each of two frames: the speed lines, and a face of its own over the
    // portrait's sprites, red in one and blue in the other (the opening's four `fgfx-*` of Goku's portrait and his
    // speed lines, a Goku of their own each)
    let dir = tempfile::tempdir().unwrap();
    // (the picture's art a gradient, so that every pixel of it is told apart)
    write_png(&dir.path().join("art/p.png"), 128, 128, |x, y| [x as u8, y as u8, 100, 255]);
    let (one, pose_one) = portrait_with_lines([3, 4]);
    let (two, pose_two) = portrait_with_lines([2, 5]);
    assert_ne!(pose_one.key, pose_two.key, "the lines make another cluster");
    // (the art covers the cluster's opaque pixels, as the pack's does)
    let cluster = |pose: &Pose, name: &str, colour: [u8; 4]| {
        let art = format!("art/{name}.png");
        let opaque = |x: u32, y: u32| pose.arr[(y / 4) as usize * pose.w + (x / 4) as usize] != 0;
        write_png(&dir.path().join(&art), pose.w as u32 * 4, pose.h as u32 * 4, |x, y| {
            if opaque(x, y) { colour } else { [0, 0, 0, 0] }
        });
        Object { id: format!("fgfx-{name}"), ..object(pose, &art, false) }
    };
    let (red, blue) = ([200, 40, 40], [40, 40, 200]);
    let objects = vec![
        cluster(&pose_one, "one", [red[0], red[1], red[2], 255]),
        cluster(&pose_two, "two", [blue[0], blue[1], blue[2], 255]),
    ];
    let (asset, picture) = portrait_picture(&one, "art/p.png");
    let pack = pack_of(dir.path(), vec![asset], ArtIndex::new(vec![picture], objects));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let first = renderer.render_hd(&one, options(4)).unwrap();
    assert!(renderer.art_stats().poses[0].matched, "the cluster is drawn from its art");
    let second = renderer.render_hd(&two, options(4)).unwrap();
    assert!(renderer.art_stats().poses[0].matched);

    // The portrait's box is the same in both frames, and on every opaque pixel of the portrait's sprite it is what
    // the picture's art alone draws (the frame drawn with a pack of the picture only)
    let (box_one, box_two) = (portrait_box(&first), portrait_box(&second));
    assert!(box_one == box_two, "the portrait does not change with the lines");
    let (asset, picture) = portrait_picture(&one, "art/p.png");
    let only_picture = pack_of(dir.path(), vec![asset], ArtIndex::new(vec![picture], Vec::new()));
    let mut reference = Renderer::new();
    reference.set_art(Some(only_picture));
    let mut plain = Renderer::new();
    for (frame, drawn) in [(&one, &box_one), (&two, &box_two)] {
        let expected = portrait_box(&reference.render_hd(frame, options(4)).unwrap());
        // (the game's sprite at 1x on a black backdrop: its opaque pixels are the ones not black)
        let sprite = plain.render_hd(frame, options(1)).unwrap();
        let opaque: Vec<usize> = (0..expected.len())
            .filter(|&n| pixel(&sprite, 40 + n % 64 / 4, 50 + n / 64 / 4) != [0, 0, 0])
            .collect();
        assert_eq!(opaque.len(), 239 * 16, "16 x 15 pixels but the first, at 4x");
        let wrong = opaque.iter().filter(|&&n| drawn[n] != expected[n]).count();
        assert_eq!(wrong, 0, "pixels of the portrait not the picture's art alone");
    }
    // The lines left of it are the cluster's art of each frame
    let line = |drawn: &crate::HdFrame| {
        (54 * 4..63 * 4).map(|y| pixel(drawn, 33 * 4 + 1, y)).find(|&p| p == red || p == blue)
    };
    assert_eq!((line(&first), line(&second)), (Some(red), Some(blue)));
}

#[test]
fn a_cluster_of_only_a_portraits_sprites_is_drawn_from_the_art_made_for_it() {
    // The field's own art for a whole portrait (the `field-fg-*` faces): the cluster's art, not the picture's
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/p.png"), 128, 128, |_, _| [20, 200, 20, 255]);
    write_png(&dir.path().join("art/b.png"), 64, 60, |_, _| [200, 40, 40, 255]);
    let (frame, pose) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    let (asset, picture) = portrait_picture(&frame, "art/p.png");
    let own = Object { id: "fgfx-face".to_owned(), ..object(&pose, "art/b.png", false) };
    let pack = pack_of(dir.path(), vec![asset], ArtIndex::new(vec![picture], vec![own]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert!(renderer.art_stats().poses[0].matched);
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 59 * 4 + 1), [200, 40, 40], "the cluster's own art");
}

#[test]
fn only_a_cluster_drawn_from_the_art_of_its_exact_pixels_has_its_portrait_drawn_again() {
    // The portrait and its lines placed from the art made for their exact pixels: their sprites are drawn again
    // from the picture. The same cluster drawn from art found by its shape, or holding it, is not
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/p.png"), 128, 128, |_, _| [20, 200, 20, 255]);
    let (frame, pose) = portrait_with_lines([3, 4]);
    write_png(&dir.path().join("art/c.png"), pose.w as u32 * 4, pose.h as u32 * 4, |_, _| {
        [200, 40, 40, 255]
    });
    let (asset, picture) = portrait_picture(&frame, "art/p.png");
    let cluster = Object { id: "fgfx-lines".to_owned(), ..object(&pose, "art/c.png", false) };
    let pack = pack_of(dir.path(), vec![asset], ArtIndex::new(vec![picture], vec![cluster]));
    // (the frame drawn once: the art's record of its pixels says which show the picture)
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.render_hd(&frame, options(4)).unwrap();
    let art = renderer.art.as_mut().unwrap();
    let found = art.find_poses(&frame);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0.source, ArtSource::Exact);
    assert_eq!(art.portraits_among_others(&found), 0b11, "the portrait's sprite and the lines'");
    let drawn_from = |source: ArtSource| {
        let mut found = found.clone();
        found[0].0.source = source;
        art.portraits_among_others(&found)
    };
    assert_eq!(drawn_from(ArtSource::Mirror), 0b11, "the art of the exact pixels mirrored");
    assert_eq!(drawn_from(ArtSource::Shaped), 0, "art found by its shape");
    assert_eq!(drawn_from(ArtSource::Held), 0, "art holding the cluster");
}

/// A field character of palette 4 (so a "field" group, and an effect by its colours too): a
/// solid black hair of two pieces with a gap, over a body of two colours, as the field's Goku
/// is drawn. Sprites 0 and 1 are the hair (8 x 8 each, three rows of six pixels), sprite 2 the
/// body (16 x 16, colours 2 and 3 in halves) below them.
fn character_log(x: u8, y: u8) -> FrameLog {
    let mut log = log(regs());
    // The hair: character 4, colour 1 in rows 0 to 2, columns 2 to 7
    for row in 0..3 {
        log.vram[4 * 16 + row] = 0x003F;
    }
    // The body: characters 0, 1, 16, 17, colour 2 (plane 1) on the left half, colour 3 on the right
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            log.vram[tile * 16 + row] = if tile % 16 == 0 { 0xFF00 } else { 0xFFFF };
        }
    }
    let attr = 4 << 1;
    log.oam_low[0..12].copy_from_slice(&[x, y, 4, attr, x + 8, y, 4, attr, x, y + 8, 0, attr]);
    log.oam_high[0] = 0b10 << 4;
    log.cgram_start[0x80 + 4 * 16 + 1] = 0x0000; // black hair
    log.cgram_start[0x80 + 4 * 16 + 2] = 0x03E0;
    log.cgram_start[0x80 + 4 * 16 + 3] = 0x7C00;
    log
}

#[test]
fn a_dark_flat_part_of_a_character_is_not_a_shadow() {
    let frame = character_log(100, 100);
    let sprites = poses::sprites(&frame);
    let found = poses::poses(&frame, &sprites);
    assert!(
        found.iter().all(|pose| !pose.shadow && pose.excluded == 0),
        "the hair is part of the character, not a ground shadow"
    );
    // The same flat black pieces as a group of their own are no ellipse either
    let hair: Vec<&poses::Sprite> = sprites.iter().filter(|s| s.idx < 2).collect();
    assert_eq!(hair.len(), 2);
    let none = |_: &poses::Sprite, _: usize, _: u8| [0_u8; 3];
    let black = |_: &poses::Sprite, _: usize, _: u8| [0_u8, 0, 0];
    assert!(!poses::assemble(&hair, false, &black).unwrap().shadow);
    assert!(poses::assemble(&hair, true, &none).is_some());

    // Drawn with the art of the character, the hair stays the art's: nothing is drawn
    // translucent over it
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let body = found.iter().find(|pose| pose.field).expect("a field group");
    let mut art = object(body, "art/o.png", false);
    art.id = "field-goku-000".to_owned();
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![art]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // The hair's pixel (row 0, column 3 of the first hair piece) is the art's solid blue
    assert_eq!(pixel(&drawn, 103 * 4 + 1, 101 * 4 + 1), [0, 0, 255]);
    let stats = renderer.art_stats();
    assert_eq!(stats.field.art, stats.field.total);
    assert!(stats.field.total > 0);
}

#[test]
fn a_field_group_is_found_by_its_key_among_the_fields_art_only() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/f.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/b.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let (frame, pose) = fighter_log(40, 50);
    // The art of a field group and the art of a battle pose have the same key (the same palette
    // indexes): a field group draws the field's, a fighter the battle's
    let mut field = object(&pose, "art/f.png", false);
    field.id = "field-npc-000".to_owned();
    let battle = object(&pose, "art/b.png", false);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![field.clone(), battle.clone()]),
    );
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [255, 0, 0], "the fighter's art");
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.art, stats.field.art), (256, 256));
    // A group of palette 4, the same indexes: the field's art
    let mut frame = frame;
    frame.oam_low[3] = 4 << 1;
    frame.cgram_start[0x80 + 4 * 16 + 2] = 0x03E0;
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "the field's art");
    let stats = renderer.art_stats();
    assert_eq!((stats.fighters.total, stats.field.art, stats.field.total), (0, 256, 256));
}

#[test]
fn a_field_frame_draws_a_group_from_the_fields_art_though_a_fighter_has_art_for_it_too() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/f.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/b.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let (frame, pose) = fighter_log(40, 50);
    let mut field = object(&pose, "art/f.png", false);
    field.id = "field-npc-000".to_owned();
    let battle = object(&pose, "art/b.png", false);
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![field, battle]));
    let kind_of_first = |known: Option<bool>| {
        let mut art = ArtState::new(Arc::clone(&pack));
        art.begin(256, 224, 4);
        art.field_frame = known;
        let found = art.find_poses(&frame);
        assert_eq!(found.len(), 1, "one of the two poses is drawn");
        found[0].0.field
    };
    // Where nothing says it, a pose of palette 1 with art made for a fighter is a fighter's
    assert!(!kind_of_first(None));
    // In a field frame it is the field's figure, drawn from the field's art
    assert!(kind_of_first(Some(true)));
}

#[test]
fn a_piece_the_edge_cuts_is_not_drawn_from_art_without_its_colours() {
    // A rock the top of the window cuts in the hidden fight was held by the all-black silhouette
    // `field-fg-9a2f097aa8` and by the white and blue ki aura `field-fg-46edb50fbe` and drawn as a
    // black blob. The same cut group as above, held by art of none of its colours: the sprite stays
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/w.png"), 64, 32, |_, _| [0, 0, 0, 255]);
    let mut frame = log(regs());
    for row in 0..8 {
        frame.vram[row] = 0xFF00;
    }
    frame.oam_low[0..4].copy_from_slice(&[0, 80, 0, 4 << 1]);
    frame.cgram_start[0x80 + 4 * 16 + 2] = 0x03E0;
    let sprites = poses::sprites(&frame);
    let found = poses::poses(&frame, &sprites);
    let cut = found.iter().find(|pose| pose.field).expect("a field group");
    assert!(cut.cut);
    let holder = |colours: &[(usize, [u8; 3])]| {
        let mut whole = object(cut, "art/w.png", false);
        whole.id = "field-fg-000".to_owned();
        whole.key ^= 1;
        whole.width = 16;
        whole.height = 8;
        whole.mask = vec![0xFF; 16];
        whole.palette = Palette::default();
        for &(index, colour) in colours {
            whole.palette.mask |= 1 << index;
            whole.palette.colors[index] = colour;
        }
        whole
    };
    let drawn_by = |whole: Object| {
        let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![whole]));
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        let matched = renderer.art_stats().poses.iter().any(|p| p.field && p.matched);
        (matched, pixel(&drawn, 2 * 4 + 1, 82 * 4 + 1))
    };
    let green = [0, 255, 0];
    assert_eq!(drawn_by(holder(&[(2, [0, 0, 0])])), (false, green), "black for green: the sprite");
    assert_eq!(
        drawn_by(holder(&[(12, [255, 255, 255])])),
        (false, green),
        "no colour 2: the sprite"
    );
    assert!(drawn_by(holder(&[(2, green)])).0, "its own green: the art");
}

#[test]
fn a_field_group_cut_by_the_edge_is_drawn_from_the_group_that_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/w.png"), 64, 32, |x, _| {
        if x < 32 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }
    });
    // A solid group of 16 x 8 pixels at the left edge: its left sprite is off the screen
    // (x = -8) and is left out of the frame's sprites, only the right one is left
    let mut frame = log(regs());
    for row in 0..8 {
        frame.vram[row] = 0xFF00;
    }
    frame.oam_low[0..4].copy_from_slice(&[0, 80, 0, 4 << 1]);
    frame.cgram_start[0x80 + 4 * 16 + 2] = 0x03E0;
    let sprites = poses::sprites(&frame);
    let found = poses::poses(&frame, &sprites);
    let cut = found.iter().find(|pose| pose.field).expect("a field group");
    assert!(cut.cut && cut.arr.iter().filter(|&&c| c != 0).count() >= 48);
    // The index holds the whole group (a 16 x 8 silhouette, made for green), not the cut one's key
    let mut whole = object(cut, "art/w.png", false);
    whole.id = "field-edge-000".to_owned();
    whole.key ^= 1;
    whole.width = 16;
    whole.height = 8;
    whole.mask = vec![0xFF; 16];
    whole.palette = Palette { mask: 0b100, ..Palette::default() };
    whole.palette.colors[2] = [0, 255, 0];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![whole]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    // The sprite's pixels are the group's left half of the silhouette it falls on (the first match
    // of the box): the art's left half, red, recoloured to the palette (which is the same green)
    let stats = renderer.art_stats();
    assert!(stats.poses.iter().any(|p| p.field && p.matched), "{:?}", stats.poses);
    assert_eq!(stats.field.art, stats.field.total);
    assert_ne!(
        pixel(&drawn, 2 * 4 + 1, 82 * 4 + 1),
        [0, 255, 0],
        "the original's pixel is replaced"
    );
    // A pose that is not at the edge is not looked for among the groups that hold it
    let mut inside = frame.clone();
    inside.oam_low[0] = 100;
    let sprites = poses::sprites(&inside);
    let found = poses::poses(&inside, &sprites);
    assert!(!found.iter().find(|pose| pose.field).unwrap().cut);
    renderer.render_hd(&inside, options(4)).unwrap();
    assert!(!renderer.art_stats().poses.iter().any(|p| p.matched));
}

/// The wide renderer's tally of `frame` with `pack`'s art: a picture frame (the game mode of a
/// battle's command screen, no field), as the tour of the game counts it.
fn tallied(pack: Arc<Pack>, frame: &FrameLog) -> crate::FrameTally {
    let mut renderer = WideRenderer::new();
    renderer.set_art(Some(pack));
    renderer.set_tally(true);
    let options = WideOptions { width: 256, hud: HudPlacement::Window };
    renderer.render_hd(Rom(&[]), 0x02, None, frame, options, self::options(1)).unwrap();
    renderer.tally()
}

#[test]
fn the_tally_says_what_the_art_draws_and_what_it_leaves() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/p.png"), 128, 128, |_, _| [255, 0, 0, 255]);
    let (mut frame, pose) = fighter_log(100, 100);
    // Three characters on BG1 at map row 1 (frame row 7): tile 1 of the portrait asset, which a
    // picture covers; tile 2 of it, which no picture does; and a character no asset has
    let (one, two, three) = (character(), [0xFF00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], {
        let mut chars = [0_u16; 16];
        chars[0] = 0xFFFF;
        chars
    });
    for (n, chars) in [one, two, three].iter().enumerate() {
        frame.vram[0x1000 + 16 * (n + 1)..0x1000 + 16 * (n + 2)].copy_from_slice(chars);
        frame.vram[0x0400 + 32 + n] = n as u16 + 1;
    }
    frame.cgram_start[1] = 0x001F;
    frame.cgram_start[2] = 0x03E0;
    frame.cgram_start[3] = 0x7C00;
    let mut keys = vec![0_u64; 16];
    keys[1] = word_key(&one);
    keys[2] = word_key(&two);
    let asset = Asset {
        id: "portrait/16:8000/4bpp".to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys,
        art: String::new(),
    };
    let mut tiles = [u16::MAX; 16];
    tiles[0] = 1;
    let art = ArtIndex::new(
        vec![Picture {
            name: "obj-00".to_owned(),
            asset: asset.id.clone(),
            tiles,
            palette: Palette::default(),
            art: "art/p.png".to_owned(),
        }],
        vec![object(&pose, "art/o.png", false)],
    );
    let tally = tallied(pack_of(dir.path(), vec![asset], art), &frame);

    let counted = |layer, asset, art, pose| -> u64 {
        tally
            .counts
            .iter()
            .filter(|c| (c.layer, c.asset, c.art, c.pose) == (layer, asset, art, pose))
            .map(|c| c.pixels)
            .sum()
    };
    // The picture draws tile 1; tile 2 is bound and has none; the pose is drawn from art
    assert_eq!(counted(TallyLayer::Bg, Some(0), true, None), 8, "tile 1 draws its picture");
    assert_eq!(counted(TallyLayer::Bg, Some(0), false, None), 8, "tile 2 has no picture");
    assert_eq!(counted(TallyLayer::Bg, None, false, None), 8, "the character no asset has");
    assert_eq!(counted(TallyLayer::Obj, None, true, Some((true, Fate::Art))), 256);
    assert_eq!(tally.pixels(), 24 + 256);
    assert_eq!(tally.art_pixels(), 8 + 256);
    // What is left: the tile with no picture and the unbound character, with their boxes
    assert_eq!(tally.items.len(), 2, "{:?}", tally.items);
    let tile = tally.items.iter().find(|i| matches!(i.key, ItemKey::Tile { .. })).unwrap();
    assert_eq!(tile.key, ItemKey::Tile { asset: 0, tile: 2, object: false });
    assert_eq!((tile.pixels, tile.bbox), (8, (8, 7, 15, 7)));
    let unbound = tally.items.iter().find(|i| matches!(i.key, ItemKey::Char { .. })).unwrap();
    let key = word_key(&[0xFFFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(unbound.key, ItemKey::Char { object: false, bpp: 4, key });
    assert_eq!(unbound.bbox, (16, 7, 23, 7));
}

#[test]
fn a_pose_the_index_lacks_is_listed_by_its_key_and_fate() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let mut other = object(&pose, "art/none.png", false);
    other.key ^= 1;
    let tally =
        tallied(pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![other])), &frame);
    assert_eq!(tally.art_pixels(), 0);
    assert_eq!(tally.items.len(), 1);
    assert_eq!(
        tally.items[0].key,
        ItemKey::Pose { key: pose.key, fighter: true, fate: Fate::Missing }
    );
    assert_eq!((tally.items[0].pixels, tally.items[0].bbox), (256, (40, 50, 55, 65)));
}

#[test]
fn a_tally_draws_no_art() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let (frame, pose) = fighter_log(40, 50);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(&pose, "art/o.png", false)]),
    );
    let mut renderer = WideRenderer::new();
    renderer.set_art(Some(pack));
    let wide = WideOptions { width: 256, hud: HudPlacement::Window };
    let with_art = renderer.render_hd(Rom(&[]), 0x02, None, &frame, wide, options(1)).unwrap();
    renderer.set_tally(true);
    let counted = renderer.render_hd(Rom(&[]), 0x02, None, &frame, wide, options(1)).unwrap();
    // The art was drawn before and is not now: the picture is the look's, the pixels counted
    assert_eq!(pixel(&with_art.picture, 41, 51), [0, 0, 255]);
    assert_eq!(pixel(&counted.picture, 41, 51), [0, 0xFF, 0]);
    assert_eq!(renderer.tally().art_pixels(), 256);
}

/// A pack with one background layer: BG4 of mode 0 (map `$0400`, characters `$1000`, one screen), the
/// tile rows 0 to 4, whose art is the colour (game x, game y, 0) of each game pixel at 4x.
pub(super) fn background_pack(dir: &std::path::Path, vram: &[u16], fill: bool) -> Arc<Pack> {
    let key = crate::bgart::layer_key(vram, 0x0400, 0x1000, 0, false, 2, [0, 4]);
    write_png(&dir.join("art/bg/test.png"), 1024, 128, |x, y| {
        [((x / 4) % 256) as u8, ((y / 4) % 256) as u8, 0, 255]
    });
    write_png(&dir.join("art/bg/test-src.png"), 256, 32, |_, _| [255, 0, 0, 255]);
    let mut pack = Arc::try_unwrap(pack_of(dir, Vec::new(), ArtIndex::default())).unwrap();
    pack.bg = crate::bgart::Index {
        layer: vec![crate::bgart::Entry {
            id: "test".to_owned(),
            key: format!("{key:016x}"),
            bg: 3,
            mode: 0,
            map_base: 0x0400,
            char_base: 0x1000,
            size: 0,
            rows: [0, 4],
            fill,
            art: "art/bg/test.png".to_owned(),
            source: "art/bg/test-src.png".to_owned(),
            palette_key: String::new(),
            palette: String::new(),
        }],
    };
    Arc::new(pack)
}

/// BG4 (mode 0) showing the solid tile 1 on every tile of its map, in red.
pub(super) fn background_frame(h_scroll: u16) -> FrameLog {
    let mut regs = regs();
    regs.bg_mode = 0;
    regs.main_bg = [false, false, false, true];
    regs.main_obj = false;
    regs.bg_map_base = [0, 0, 0, 0x0400];
    regs.bg_char_base = [0, 0, 0, 0x1000];
    regs.bg_h_scroll[3] = h_scroll;
    let mut log = log(regs);
    for word in 0..8 {
        log.vram[0x1000 + 8 + word] = 0x00FF;
    }
    for tile in 0..1024 {
        log.vram[0x0400 + tile] = 1;
    }
    log.cgram_start[0x61] = 0x001F;
    log
}

#[test]
fn the_transformations_lightning_is_drawn_over_the_art() {
    let dir = tempfile::tempdir().unwrap();
    let log = background_frame(10);
    let pack = background_pack(dir.path(), &log.vram, true);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&pack)));
    let plain = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&plain, 0, 0), [10, 1, 0], "the layer's art");
    // An opaque picture (RGBA, alpha 255) of one colour: laid over by its alpha, it covers the art
    let lightning = dir.path().join("ssj-lightning");
    write_png(&lightning.join("frame-1.png"), 1024, 896, |_, _| [0, 0, 200, 255]);
    renderer.set_lightning(crate::lightning::Lightning::load(&lightning).map(Arc::new));
    renderer.set_conditions(Conditions::default().with_ssj_lightning(None));
    assert!(renderer.render_hd(&log, options(4)).unwrap() == plain, "not in the transformation");
    renderer.set_conditions(Conditions::default().with_ssj_lightning(Some(0)));
    let lit = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&lit, 0, 0), [0, 0, 200], "over the art");
    assert_eq!(pixel(&lit, 8, 40 * 4), [0, 0, 200], "and over the tiles drawn as the look does");
}

#[test]
fn a_background_layer_is_drawn_from_its_art_at_the_scroll_of_each_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = background_frame(10);
    let pack = background_pack(dir.path(), &log.vram, true);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&pack)));
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    // The first frame row is scanline 1: the layer's row 1; the pixel 0 shows the layer's pixel 10
    assert_eq!(pixel(&picture, 0, 0), [10, 1, 0]);
    assert_eq!(pixel(&picture, 3, 3), [10, 1, 0]);
    assert_eq!(pixel(&picture, 4, 4), [11, 2, 0]);
    // Past the art's rows (32 pixels) the tiles are drawn as the look does
    let below = pixel(&picture, 8, 40 * 4);
    assert_eq!(below, [255, 0, 0]);
    // The scroll wraps with the layer, and each line has its own: line 100 scrolls by 250
    for entry in &mut log.entries[99..] {
        entry.regs.bg_h_scroll[3] = 250;
    }
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 0, 0), [10, 1, 0]);
    assert_eq!(renderer.art_stats().bg_pixels, 0, "the layer is not a battle background");
    // A pixel at line 10 (frame row 9)
    let again = pixel(&picture, 8, 9 * 4);
    assert_eq!(again, [12, 10, 0]);
    // The pixel after the wrap: x 6 of a line that scrolls 250 shows the layer's pixel 0
    let mut wrapped = background_frame(250);
    wrapped.vram.copy_from_slice(&log.vram);
    let picture = renderer.render_hd(&wrapped, options(4)).unwrap();
    assert_eq!(pixel(&picture, 6 * 4, 0), [0, 1, 0]);
}

#[test]
fn a_faded_palette_dims_the_background_art_by_the_same_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = background_frame(0);
    let pack = background_pack(dir.path(), &log.vram, true);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    // The game's red is half as bright (the art was made for full red): art colour (100, 0, 0)
    log.cgram_start[0x61] = 0x000F;
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    // game x 100, row 1: art (100, 1, 0); the source red 255 became 123: a fade, drawn as a plain
    // blend of the art towards black
    let [r, g, _] = pixel(&picture, 100 * 4, 0);
    assert!((46..=50).contains(&r), "red {r}");
    assert!(g <= 1, "green {g}");
}

#[test]
fn art_made_for_the_palette_in_use_is_bound_before_the_art_of_the_content() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = background_frame(0);
    let mut pack = Arc::try_unwrap(background_pack(dir.path(), &log.vram, true)).unwrap();
    // A variant of the same content made for the green palette: its art is blue
    write_png(&dir.path().join("art/bg/green.png"), 1024, 128, |_, _| [0, 0, 200, 255]);
    write_png(&dir.path().join("art/bg/green-src.png"), 256, 32, |_, _| [0, 255, 0, 255]);
    let key = |log: &FrameLog| {
        crate::bgart::palette_key(
            &log.vram,
            &log.cgram_start,
            3,
            0,
            0x0400,
            0,
            false,
            2,
            [0, 4],
            true,
        )
    };
    let mut green_log = log.clone();
    green_log.cgram_start[0x61] = 0x03E0;
    let mut variant = pack.bg.layer[0].clone();
    variant.id = "green".to_owned();
    variant.art = "art/bg/green.png".to_owned();
    variant.source = "art/bg/green-src.png".to_owned();
    variant.palette_key = format!("{:016x}", key(&green_log));
    // (the variant is listed first: the order of the index does not decide)
    pack.bg.layer.insert(0, variant);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack)));
    // The green palette: the variant's art as it is
    let picture = renderer.render_hd(&green_log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 100 * 4, 0), [0, 0, 200]);
    // The palette of the base art: the base art
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 100 * 4, 0), [100, 1, 0]);
    // A palette nobody made art for: the base art, in the colours in use (not the variant's)
    log.cgram_start[0x61] = 0x7C00;
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    let [r, _, b] = pixel(&picture, 100 * 4, 0);
    assert!(b > r, "blue palette, base art recoloured: {r} {b}");
    assert!(b != 200);
}

#[test]
fn a_palette_the_art_was_made_in_recolours_it_as_the_source_picture_does() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = background_frame(0);
    let with_source = background_pack(dir.path(), &log.vram, true);
    // The same pack with no source picture, but the CGRAM the art was made in: entry 0x61 red
    let mut pack = Arc::try_unwrap(background_pack(dir.path(), &log.vram, true)).unwrap();
    pack.bg.layer[0].source = String::new();
    let mut made_in = [0_u16; 256];
    made_in[0x61] = 0x001F;
    pack.bg.layer[0].palette = crate::bgart::palette_text(&made_in);
    assert_eq!(crate::bgart::parse_palette(&pack.bg.layer[0].palette), Some(made_in));
    let mut by_source = Renderer::new();
    by_source.set_art(Some(with_source));
    let mut by_palette = Renderer::new();
    by_palette.set_art(Some(Arc::new(pack)));
    // The palette the art was made in, a faded one and a flash to another hue: the same pictures
    for word in [0x001F_u16, 0x000F, 0x7C00, 0x03E0] {
        log.cgram_start[0x61] = word;
        let a = by_source.render_hd(&log, options(4)).unwrap();
        let b = by_palette.render_hd(&log, options(4)).unwrap();
        assert_eq!(a.rgba, b.rgba, "palette word {word:04X}");
    }
}

#[test]
fn a_palette_text_is_256_words_of_four_hex_digits() {
    let mut colours = [0_u16; 256];
    colours[3] = 0x7FFF;
    colours[255] = 0x0123;
    assert_eq!(crate::bgart::parse_palette(&crate::bgart::palette_text(&colours)), Some(colours));
    assert_eq!(crate::bgart::parse_palette(""), None);
    assert_eq!(crate::bgart::parse_palette(&"G".repeat(1024)), None);
    assert_eq!(crate::bgart::parse_palette(&"0".repeat(1020)), None);
}

#[test]
fn a_background_pack_without_a_source_picture_draws_the_art_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = background_frame(0);
    let mut pack = Arc::try_unwrap(background_pack(dir.path(), &log.vram, true)).unwrap();
    pack.bg.layer[0].source = String::new();
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack)));
    // The palette the art was made for, and a faded one: no source, so no recolouring either way
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 100 * 4, 0), [100, 1, 0]);
    log.cgram_start[0x61] = 0x000F;
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 100 * 4, 0), [100, 1, 0]);
}

#[test]
fn a_frame_whose_layer_is_not_in_the_index_is_drawn_as_the_look() {
    let dir = tempfile::tempdir().unwrap();
    let log = background_frame(0);
    let pack = background_pack(dir.path(), &log.vram, true);
    let mut other = log.clone();
    other.vram[0x0400] = 0; // another map: another content
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let picture = renderer.render_hd(&other, options(4)).unwrap();
    assert_eq!(pixel(&picture, 40, 40), [255, 0, 0]);
    // and in the same renderer the known layer is still found
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 40, 40), [10, 11, 0]);
}

/// A Mode 7 frame: BG1 alone, the identity matrix, every tile of the plane the tile 1 whose pixels
/// are the colour index 5 (CGRAM 5 is `0x001F`, full red).
pub(crate) fn plane_frame() -> FrameLog {
    let mut regs = regs();
    regs.bg_mode = 7;
    regs.main_obj = false;
    regs.m7_a = 0x0100;
    regs.m7_d = 0x0100;
    let mut log = log(regs);
    for word in &mut log.vram[..128 * 128] {
        *word = 1;
    }
    for pixel in 0..64 {
        log.vram[64 + pixel] = 5 << 8;
    }
    log.cgram_start[5] = 0x001F;
    log
}

/// A pack with the art of the plane of `vram`: the colour (game x / 4, game y / 4, 77) of each game
/// pixel at 4x, made in the colour 5 = full red.
pub(crate) fn plane_pack(dir: &std::path::Path, vram: &[u16]) -> Arc<Pack> {
    plane_pack_with(dir, vram, |_, _| 255)
}

/// [`plane_pack`] where the art of the tile `(column, row)` has the alpha `alpha`.
fn plane_pack_with(
    dir: &std::path::Path,
    vram: &[u16],
    alpha: impl Fn(u32, u32) -> u8,
) -> Arc<Pack> {
    use crate::mode7art;
    for ty in 0..4_u32 {
        for tx in 0..4_u32 {
            write_png(&dir.join(format!("art/mode7/test/t{tx}_{ty}.png")), 1024, 1024, |x, y| {
                [
                    (((tx * 1024 + x) / 16) % 256) as u8,
                    (((ty * 1024 + y) / 16) % 256) as u8,
                    77,
                    alpha(tx, ty),
                ]
            });
        }
    }
    let mut pack = Arc::try_unwrap(pack_of(dir, Vec::new(), ArtIndex::default())).unwrap();
    pack.mode7 = mode7art::Index {
        plane: vec![mode7art::Entry {
            id: "test".to_owned(),
            key: format!("{:016x}", mode7art::plane_key(vram)),
            tiles: "test/t{x}_{y}.png".to_owned(),
            palette: vec!["FF0000".to_owned(); 256],
            replace: false,
        }],
    };
    Arc::new(pack)
}

#[test]
fn the_mode_7_plane_is_drawn_from_its_art_through_the_matrix_of_each_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = plane_frame();
    let pack = plane_pack(dir.path(), &log.vram);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&pack)));
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    // Picture pixel (36, 40): the plane's pixel (9, 11) (the first frame row is scanline 1): the art
    // there is (9 * 4 / 16, 11 * 4 / 16) = (2, 2) in a block of 16 art pixels
    assert_eq!(pixel(&picture, 36, 40), [2, 2, 77]);
    assert_eq!(pixel(&picture, 100, 100), [6, 6, 77]);
    // A line that zooms out by two (a = d = 0x200) from line 100 shows the plane twice as far along
    for entry in &mut log.entries[99..] {
        entry.regs.m7_a = 0x0200;
        entry.regs.m7_d = 0x0200;
    }
    let picture = renderer.render_hd(&log, options(4)).unwrap();
    assert_eq!(pixel(&picture, 36, 40), [2, 2, 77], "the lines before the change are as they were");
    // Row 400 is line 101: plane pixel (2 * 25, 2 * 101) at picture column 100
    assert_eq!(pixel(&picture, 100, 400), [12, 50, 77]);
}

#[test]
fn a_faded_plane_dims_the_art_and_an_unknown_plane_is_the_look() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = plane_frame();
    let pack = plane_pack(dir.path(), &log.vram);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&pack)));
    // The game's red is half as bright: the art (2, 2, 77) takes the same ratio
    log.cgram_start[5] = 0x000F;
    let [r, g, b] = pixel(&renderer.render_hd(&log, options(4)).unwrap(), 36, 40);
    assert!((38..=44).contains(&b), "blue {b}");
    assert!(r < 140 && g <= 4, "{r} {g}");
    // Another plane: the colour the game shows (red)
    let mut other = plane_frame();
    other.vram[0] = 2;
    let picture = renderer.render_hd(&other, options(4)).unwrap();
    assert_eq!(pixel(&picture, 36, 40), [255, 0, 0]);
}

#[test]
fn a_flight_continues_the_plane_art_past_the_window_in_the_wide_view() {
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame();
    let pack = plane_pack(dir.path(), &log.vram);
    let mut wide = WideRenderer::new();
    wide.set_art(Some(pack));
    let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
    let view = wide.render_hd(Rom(&[]), 0x07, None, &log, options_wide, options(2)).unwrap();
    assert_eq!(view.scene, crate::world::Scene::Flight);
    // The window starts at column 58 of the wide frame (116 picture pixels). Picture column 160 is
    // window pixel 22 (2 picture pixels each): the art (22 * 4 / 16 = 5, 5) is at row 40 (plane y 21)
    assert_eq!(pixel(&view.picture, 160, 40), [5, 5, 77]);
    // Column 20 is window pixel -48: the plane wraps around (1024 - 48 = 976: 976 * 4 / 16 = 244)
    assert_eq!(pixel(&view.picture, 20, 40), [244, 5, 77]);
}

#[test]
fn a_texel_of_the_plane_without_art_is_the_look() {
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame();
    // The tile 0 of the art (plane pixels 0 to 255) has no art: the game's red shows there
    let pack =
        plane_pack_with(dir.path(), &log.vram, |tx, ty| if (tx, ty) == (0, 0) { 0 } else { 255 });
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    assert_eq!(pixel(&renderer.render_hd(&log, options(4)).unwrap(), 36, 40), [255, 0, 0]);
}

/// [`plane_frame`] with other pixels in the character 2, which no map entry uses: what a battle's opening flight left
/// by the battle before has in VRAM (the plane shows the same, its `plane_key` is another).
fn plane_frame_with_other_unused_characters() -> FrameLog {
    let mut log = plane_frame();
    // (the low bytes are the map's, the tile 1 everywhere)
    for pixel in 0..64 {
        log.vram[128 + pixel] = 9 << 8 | 1;
    }
    log
}

#[test]
fn a_plane_shown_with_other_unused_characters_is_drawn_from_the_art_of_what_it_shows() {
    use crate::mode7art::{content_key, plane_key};
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame();
    let other = plane_frame_with_other_unused_characters();
    assert_ne!(plane_key(&other.vram), plane_key(&log.vram));
    assert_eq!(content_key(&other.vram), content_key(&log.vram));
    // The pack's art keyed by the content of `log` (as an art override's plane is)
    let mut pack = (*plane_pack(dir.path(), &log.vram)).clone();
    let plane_key_of_log = pack.mode7.plane[0].key.clone();
    pack.mode7.plane[0].key =
        format!("{}{:016x}", crate::mode7art::CONTENT_PREFIX, content_key(&log.vram));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack.clone())));
    assert_eq!(pixel(&renderer.render_hd(&other, options(4)).unwrap(), 36, 40), [2, 2, 77]);
    assert_eq!(pixel(&renderer.render_hd(&log, options(4)).unwrap(), 36, 40), [2, 2, 77]);
    // The same content in another colour (yellow, not the art's red): the game's pixels, the art not recoloured
    let mut yellow = other.clone();
    yellow.cgram_start[5] = 0x03FF;
    assert_eq!(pixel(&renderer.render_hd(&yellow, options(4)).unwrap(), 36, 40), [255, 255, 0]);
    // Keyed by the plane key of `log`, the art is found for it only, as before: the game's red for `other`
    pack.mode7.plane[0].key = plane_key_of_log;
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack)));
    assert_eq!(pixel(&renderer.render_hd(&other, options(4)).unwrap(), 36, 40), [255, 0, 0]);
    assert_eq!(pixel(&renderer.render_hd(&log, options(4)).unwrap(), 36, 40), [2, 2, 77]);
}

#[test]
fn a_plane_of_the_art_override_is_drawn_for_its_content_before_the_packs_own_art() {
    use crate::mode7art::content_key;
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame();
    let other = plane_frame_with_other_unused_characters();
    let pack = (*plane_pack(dir.path(), &log.vram)).clone();
    // The override's plane: the tile t0_0 only (plane pixels 0 to 255; the other tiles have no file), (10, 20, 30) but
    // transparent left of plane column 128; no palette.txt and the pack has no art of this content: left out
    let overrides = tempfile::tempdir().unwrap();
    let folder = overrides
        .path()
        .join(crate::pack::MODE7_DIR)
        .join(format!("{:016x}", content_key(&log.vram)));
    write_png(&folder.join("t0_0.png"), 1024, 1024, |x, _| {
        [10, 20, 30, if x < 512 { 0 } else { 255 }]
    });
    let refused = pack.clone().with_art_override(overrides.path());
    assert!(refused.mode7.plane.iter().all(|entry| entry.content_value().is_none()));
    // With its palette (the game's red at 5, as the art was made in): drawn for both VRAMs, before the pack's own art
    let mut palette = vec!["000000"; 256];
    palette[5] = "FF0000";
    std::fs::write(folder.join("palette.txt"), palette.join("\n")).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack.clone().with_art_override(overrides.path()))));
    // (plane pixel (150, 11) at picture (600, 40); plane pixel (9, 11), where the art is transparent, the game's red)
    assert_eq!(pixel(&renderer.render_hd(&log, options(4)).unwrap(), 600, 40), [10, 20, 30]);
    assert_eq!(pixel(&renderer.render_hd(&other, options(4)).unwrap(), 600, 40), [10, 20, 30]);
    assert_eq!(pixel(&renderer.render_hd(&log, options(4)).unwrap(), 36, 40), [255, 0, 0]);
    // A tile of another size, or a folder whose name is no content key, is left out
    write_png(&folder.join("t1_0.png"), 512, 512, |_, _| [0, 0, 0, 255]);
    let wrong = pack.clone().with_art_override(overrides.path());
    assert!(wrong.mode7.plane.iter().all(|entry| entry.content_value().is_none()));
    std::fs::remove_file(folder.join("t1_0.png")).unwrap();
    std::fs::rename(&folder, overrides.path().join(crate::pack::MODE7_DIR).join("super-vegeta"))
        .unwrap();
    let unnamed = pack.with_art_override(overrides.path());
    assert!(unnamed.mode7.plane.iter().all(|entry| entry.content_value().is_none()));
}

/// The messages logged since the test process started (a logger that keeps them, set by the first call).
fn captured_log() -> Vec<String> {
    struct Keep(std::sync::Mutex<Vec<String>>);
    impl log::Log for Keep {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        fn flush(&self) {}
    }
    static KEEP: Keep = Keep(std::sync::Mutex::new(Vec::new()));
    if log::set_logger(&KEEP).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    KEEP.0.lock().unwrap().clone()
}

#[test]
fn an_override_plane_with_replace_shows_what_is_under_the_plane_where_its_art_is_transparent() {
    use crate::mode7art::content_key;
    let _ = captured_log();
    let dir = tempfile::tempdir().unwrap();
    let mut log = plane_frame();
    // Under the plane (all red, colour 5): the backdrop, green
    log.cgram_start[0] = 0x03E0;
    let pack = (*plane_pack(dir.path(), &log.vram)).clone();
    // The override's plane: the tile t0_0 only, transparent left of plane column 128, (10, 20, 30) right of it
    let overrides = tempfile::tempdir().unwrap();
    let folder = overrides
        .path()
        .join(crate::pack::MODE7_DIR)
        .join(format!("{:016x}", content_key(&log.vram)));
    write_png(&folder.join("t0_0.png"), 1024, 1024, |x, _| {
        [10, 20, 30, if x < 512 { 0 } else { 255 }]
    });
    let mut palette = vec!["000000"; 256];
    palette[5] = "FF0000";
    std::fs::write(folder.join("palette.txt"), palette.join("\n")).unwrap();
    // Picture pixels (36, 40) (plane pixel (9, 11): the art transparent) and (600, 40) (plane pixel (150, 11): the
    // art), and the frames drawn again without BG1, in the Classic picture and in the wide view (window pixel 22 at
    // picture column 160, the art transparent there)
    let draw = |pack: &Pack| {
        let mut renderer = Renderer::new();
        renderer.set_art(Some(Arc::new(pack.clone())));
        let picture = renderer.render_hd(&log, options(4)).unwrap();
        let mut wide = WideRenderer::new();
        wide.set_art(Some(Arc::new(pack.clone())));
        let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
        let view = wide.render_hd(Rom(&[]), 0x07, None, &log, options_wide, options(2)).unwrap();
        (
            pixel(&picture, 36, 40),
            pixel(&picture, 600, 40),
            renderer.art_stats().plane_under,
            pixel(&view.picture, 160, 40),
            wide.art_stats().plane_under,
        )
    };
    let (red, green, art) = ([255, 0, 0], [0, 255, 0], [10, 20, 30]);
    // Without `replace`: where the art is transparent, the game's red; the frame drawn once
    let plain = pack.clone().with_art_override(overrides.path());
    assert!(plain.mode7.plane.iter().all(|entry| !entry.replace));
    assert_eq!(draw(&plain), (red, art, 0, red, 0));
    // A file of another name (`Replace`, `replace.txt`) is not `replace`: left out, and the log says so
    std::fs::write(folder.join("Replace"), "").unwrap();
    std::fs::write(folder.join("replace.txt"), "").unwrap();
    let misnamed = pack.clone().with_art_override(overrides.path());
    assert!(misnamed.mode7.plane.iter().all(|entry| !entry.replace));
    let logged = captured_log();
    for name in ["Replace", "replace.txt"] {
        let shown = format!("{} is left out", folder.join(name).display());
        let said = format!("{}: {name} is left out", folder.display());
        assert!(
            logged.iter().any(|line| line.contains(&said) || line.contains(&shown)),
            "{name}: {logged:?}"
        );
    }
    assert_eq!(draw(&misnamed), (red, art, 0, red, 0));
    // With `replace`: what is under the plane there (the backdrop), the frame drawn again without BG1 once
    std::fs::remove_file(folder.join("Replace")).unwrap();
    std::fs::write(folder.join("replace"), "").unwrap();
    let replaced = pack.clone().with_art_override(overrides.path());
    assert!(replaced.mode7.plane.iter().any(|entry| entry.replace));
    assert_eq!(draw(&replaced), (green, art, 1, green, 1));
    // The pack's own art: as before, the frame drawn once
    let (own, _, again, _, again_wide) = draw(&pack);
    assert_eq!((own, again, again_wide), ([2, 2, 77], 0, 0));
}

/// [`plane_frame`] whose tile shows the colour 5 in the left four columns and is transparent (colour 0) in the right
/// four, over a green backdrop, with the main screen's BG1 window switched on but neither window set for BG1 (what
/// the opening flights of Super Saiyan Goku and Super Vegeta do): the window hides nothing. Plane column `x` is
/// transparent when `x % 8 >= 4`.
fn plane_frame_with_an_idle_bg1_window() -> FrameLog {
    let mut log = plane_frame();
    for row in 0..8 {
        for column in 4..8 {
            log.vram[64 + row * 8 + column] = 0;
        }
    }
    log.cgram_start[0] = 0x03E0;
    for entry in &mut log.entries {
        entry.regs.main_bg_window_off[0] = true;
    }
    log
}

/// The art override of the plane of `log` in `overrides`: the tiles `t0_0` to `t3_0` (plane rows 0..256, every column, so
/// the wide view's margins, which wrap round the plane, have art too), (10, 20, 30) where `opaque(plane x)`, made in the
/// game's red (colour 5); with the empty file `replace` when `replace`.
fn override_plane(
    overrides: &std::path::Path,
    log: &FrameLog,
    opaque: impl Fn(u32) -> bool,
    replace: bool,
) {
    let folder = overrides
        .join(crate::pack::MODE7_DIR)
        .join(format!("{:016x}", crate::mode7art::content_key(&log.vram)));
    for tx in 0..4_u32 {
        write_png(&folder.join(format!("t{tx}_0.png")), 1024, 1024, |x, _| {
            [10, 20, 30, if opaque(tx * 256 + x / 4) { 255 } else { 0 }]
        });
    }
    let mut palette = vec!["000000"; 256];
    palette[5] = "FF0000";
    std::fs::write(folder.join("palette.txt"), palette.join("\n")).unwrap();
    if replace {
        std::fs::write(folder.join("replace"), "").unwrap();
    }
}

/// The picture pixels of window pixel `x` (frame row 10) in the Classic picture (4x) and the wide view (2x).
fn plane_pixels(pack: &Pack, log: &FrameLog, x: usize) -> [[u8; 3]; 2] {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new(pack.clone())));
    let picture = renderer.render_hd(log, options(4)).unwrap();
    let mut wide = WideRenderer::new();
    wide.set_art(Some(Arc::new(pack.clone())));
    let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
    let view = wide.render_hd(Rom(&[]), 0x07, None, log, options_wide, options(2)).unwrap();
    [pixel(&picture, x * 4 + 1, 41), pixel(&view.picture, (58 + x) * 2 + 1, 21)]
}

/// The wide view's picture pixel (2x) of window pixel `x` (frame row 10), which may be in a margin (`x` < 0 or > 255).
fn wide_plane_pixel(pack: &Pack, log: &FrameLog, x: i32) -> [u8; 3] {
    let mut wide = WideRenderer::new();
    wide.set_art(Some(Arc::new(pack.clone())));
    let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
    let view = wide.render_hd(Rom(&[]), 0x07, None, log, options_wide, options(2)).unwrap();
    pixel(&view.picture, ((58 + x) * 2 + 1) as usize, 21)
}

/// What the art of a plane must do under the BG1 window, for `pack` (whose art `is_art` tells from the game's red and
/// the backdrop) and the frame of [`plane_frame_with_an_idle_bg1_window`]: drawn where the switch is on with no window
/// set for BG1 (also past the game's outline); hidden inside a window set for BG1, over the whole line or part of it;
/// drawn in the wide view's margins as the game's layer is there.
fn the_plane_art_follows_the_bg1_window(
    pack: &Pack,
    is_art: impl Fn([u8; 3]) -> bool,
    whose: &str,
) {
    let log = plane_frame_with_an_idle_bg1_window();
    let art = |colours: [[u8; 3]; 2]| colours.iter().all(|&colour| is_art(colour));
    // Where the game's plane shows its red and where it is transparent (past the outline): the art, not the
    // backdrop of a pixel taken as hidden by the window
    assert!(art(plane_pixels(pack, &log, 2)), "{whose}: the art");
    assert!(art(plane_pixels(pack, &log, 6)), "{whose}: the art past the game's outline");
    // A window set for BG1 over the whole line does hide the plane there: the backdrop, no art
    let mut covered = log.clone();
    for entry in &mut covered.entries {
        entry.regs.bg_window_area[0][0] = 1;
        entry.regs.window_left[0] = 0;
        entry.regs.window_right[0] = 255;
    }
    assert_eq!(plane_pixels(pack, &covered, 6), [[0, 255, 0]; 2], "{whose}: inside a window");
    // A window over part of the line (window pixels 0..=3) hides the plane there only
    let mut part = log.clone();
    for entry in &mut part.entries {
        entry.regs.bg_window_area[0][0] = 1;
        entry.regs.window_left[0] = 0;
        entry.regs.window_right[0] = 3;
    }
    assert_eq!(
        plane_pixels(pack, &part, 2),
        [[0, 255, 0]; 2],
        "{whose}: inside the window: the backdrop"
    );
    assert!(art(plane_pixels(pack, &part, 6)), "{whose}: outside it: the art");
    // In the wide view's margins no window is inside (`screen::inside`): the game's BG1 shows there (its red at margin
    // pixel -14, plane column 2 of the wrapped plane), and the art is drawn there as the layer is (at -10, column 6,
    // where the plane is transparent), not taken as hidden by the window over the original's 256 pixels
    let mut bare = pack.clone();
    bare.mode7 = crate::mode7art::Index::default();
    assert_eq!(
        wide_plane_pixel(&bare, &covered, -14),
        [255, 0, 0],
        "the game's layer in the margin"
    );
    assert!(
        is_art(wide_plane_pixel(pack, &covered, -10)),
        "{whose}: the art in the margin, as the layer"
    );
}

#[test]
fn a_planes_art_past_the_games_outline_is_drawn_when_the_bg1_window_hides_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame_with_an_idle_bg1_window();
    // The pack's own art (blue 77), found by the plane key and by the plane's content (a `content:` key)
    let pack = (*plane_pack(dir.path(), &log.vram)).clone();
    let packs_art = |colour: [u8; 3]| colour[2] == 77;
    the_plane_art_follows_the_bg1_window(&pack, packs_art, "the pack's plane");
    let mut by_content = pack.clone();
    by_content.mode7.plane[0].key = format!(
        "{}{:016x}",
        crate::mode7art::CONTENT_PREFIX,
        crate::mode7art::content_key(&log.vram)
    );
    the_plane_art_follows_the_bg1_window(
        &by_content,
        packs_art,
        "the pack's plane found by its content",
    );
    // A picture of the art override, (10, 20, 30)
    let overrides = tempfile::tempdir().unwrap();
    override_plane(overrides.path(), &log, |_| true, false);
    let with = pack.with_art_override(overrides.path());
    the_plane_art_follows_the_bg1_window(
        &with,
        |colour| colour == [10, 20, 30],
        "the art override's plane",
    );
}

#[test]
fn an_override_plane_with_replace_shows_what_is_under_it_when_the_bg1_window_hides_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let log = plane_frame_with_an_idle_bg1_window();
    let pack = (*plane_pack(dir.path(), &log.vram)).clone();
    let overrides = tempfile::tempdir().unwrap();
    // The art is clear over the game's red columns and opaque over the transparent ones
    override_plane(overrides.path(), &log, |x| x % 8 >= 4, true);
    let with = pack.with_art_override(overrides.path());
    assert!(with.mode7.plane.iter().any(|entry| entry.replace));
    // Where the art is clear and the game's plane red: what is under the plane (the green backdrop), not the red
    assert_eq!(
        plane_pixels(&with, &log, 2),
        [[0, 255, 0]; 2],
        "under the plane, not the game's pixel"
    );
    // Where the game's plane is transparent and the art opaque: the art
    assert_eq!(plane_pixels(&with, &log, 6), [[10, 20, 30]; 2], "the art past the game's outline");
}

/// A fighter of one 16 x 16 sprite whose tiles hold the left four columns of colour 2 (so its
/// pose is not its own mirror image), drawn flipped left to right or not.
fn lopsided_fighter(flipped: bool) -> (FrameLog, Pose) {
    let (mut log, _) = fighter_log(40, 50);
    // The left tiles hold four columns, the right ones two
    for (tile, word) in [(0_usize, 0xF000_u16), (16, 0xF000), (1, 0xC000), (17, 0xC000)] {
        for row in 0..8 {
            log.vram[tile * 16 + row] = word;
        }
    }
    if flipped {
        log.oam_low[3] |= 0x40;
    }
    let sprites = poses::sprites(&log);
    let found: Vec<Pose> =
        poses::poses(&log, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(found.len(), 1);
    (log, found.into_iter().next().unwrap())
}

#[test]
fn a_fighter_drawn_flipped_is_drawn_from_the_art_of_its_mirror_image_mirrored() {
    let dir = tempfile::tempdir().unwrap();
    let (plain_log, plain) = lopsided_fighter(false);
    let (flipped_log, flipped) = lopsided_fighter(true);
    assert_ne!(plain.key, flipped.key, "the two facings are two poses");
    assert_eq!(plain.mirror_key(), Some(flipped.key));
    // The art of the pose facing right: red on its left half, blue on its right half
    let half = (plain.w * 4 / 2) as u32;
    write_png(&dir.path().join("art/o.png"), (plain.w * 4) as u32, (plain.h * 4) as u32, |x, _| {
        if x < half { [255, 0, 0, 255] } else { [0, 0, 255, 255] }
    });
    let mut held = object(&plain, "art/o.png", false);
    held.palette = Palette { mask: 0b100, ..Palette::default() };
    held.palette.colors[2] = [0, 255, 0];
    held.mask = vec![0xFF; 32];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![held]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));

    // The pose as the index has it: its left columns show the art's left (red)
    let drawn = renderer.render_hd(&plain_log, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 40 * 4 + 1, 55 * 4 + 1), [255, 0, 0]);
    assert_eq!(renderer.art_stats().poses[0].source, crate::ArtSource::Exact);

    // Flipped, it has no art of its own: the mirrored art is drawn, its columns where the
    // flipped sprite's are (the box starts at the art's right end: blue, and ends red)
    let drawn = renderer.render_hd(&flipped_log, options(4)).unwrap();
    let stats = renderer.art_stats();
    assert!(stats.poses[0].matched);
    assert_eq!(stats.poses[0].source, crate::ArtSource::Mirror);
    assert_eq!(pixel(&drawn, 46 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
    assert_eq!(pixel(&drawn, 52 * 4 + 1, 55 * 4 + 1), [255, 0, 0]);
}

#[test]
fn units_of_one_pose_are_one_unit_in_a_frame() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/a.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/b.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    // Two fighters of the same pixels, one in each fighter palette (a formation of one enemy
    // whose units are told apart by the colours)
    let (mut frame, pose) = fighter_log(40, 50);
    frame.oam_low[4..8].copy_from_slice(&[120, 50, 0, 2 << 1]);
    frame.oam_high[0] = 0b1010;
    frame.cgram_start[0x80 + 32 + 2] = 0x7D00; // palette 2, colour 2: blue-ish
    let unit = |id: &str, art: &str, colour: [u8; 3]| {
        let mut held = object(&pose, art, false);
        held.id = id.to_owned();
        held.palette = Palette { mask: 0b100, ..Palette::default() };
        held.palette.colors[2] = colour;
        held.mask = vec![0xFF; 32];
        held
    };
    // Unit a was made for green and unit b for blue: the first fighter (green) is a's by colour,
    // the second (blue) is b's by colour, and the frame draws both from the unit that fits best
    let index = ArtIndex::new(
        Vec::new(),
        vec![
            unit("battle-a-000", "art/a.png", [0, 255, 0]),
            unit("battle-b-000", "art/b.png", [0, 0, 255]),
        ],
    );
    let pack = pack_of(dir.path(), Vec::new(), index);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let fighters: Vec<_> = stats.poses.iter().filter(|p| p.fighter).collect();
    assert_eq!(fighters.len(), 2);
    assert_eq!(fighters[0].art, Some(0), "unit a's art");
    assert_eq!(
        fighters[0].art, fighters[1].art,
        "the two fighters are drawn from the art of one unit"
    );
    assert!(fighters[1].recoloured, "in the colours of its own palette");
}

#[test]
fn a_fighter_drawn_with_a_later_palette_is_still_a_fighter_in_a_frame_of_fighters() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    // The game draws the third enemy of a formation with the palette after the two fighter ones
    let (mut frame, pose) = fighter_log(40, 50);
    frame.oam_low[3] = 3 << 1;
    frame.cgram_start[0x80 + 3 * 16 + 2] = 0x03E0;
    let mut held = object(&pose, "art/o.png", false);
    held.palette = Palette { mask: 0b100, ..Palette::default() };
    held.palette.colors[2] = [0, 255, 0];
    held.mask = vec![0xFF; 32];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![held]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    // Alone, a group of that palette is the field's (or an effect's), not a fighter
    renderer.render_hd(&frame, options(4)).unwrap();
    assert!(!renderer.art_stats().poses.iter().any(|p| p.fighter && p.palette == 3));
    // Beside a fighter of palette 1 that has art, it is the formation's third unit
    frame.oam_low[4..8].copy_from_slice(&[120, 50, 0, 1 << 1]);
    frame.oam_high[0] = 0b1010;
    frame.cgram_start[0x80 + 16 + 2] = 0x03E0;
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 55 * 4 + 1), [0, 0, 255]);
    let stats = renderer.art_stats();
    assert!(
        stats.poses.iter().any(|p| p.fighter && p.palette == 3 && p.matched),
        "{:?}",
        stats.poses
    );
}

/// A frame of 239 lines (the overscan some scenes use) with one effect sprite (palette 4) of 8 x 8
/// pixels in colour 1 (white) at `(x, y)`, the OAM `y` being 224 or more.
fn overscan_log(x: u8, y: u8) -> FrameLog {
    let mut log = log(regs());
    log.height = 239;
    log.entries = (1..=239_u16)
        .map(|scanline| LineEntry {
            scanline,
            from_pixel: 0,
            regs: regs(),
            sprites: SpriteLine { eval_line: scanline - 1, rebuilt: true, ..SpriteLine::default() },
        })
        .collect();
    log.oam_low[0] = x;
    log.oam_low[1] = y;
    log.oam_low[3] = 4 << 1;
    for row in 0..8 {
        log.vram[row] = 0x00FF;
    }
    log.cgram_start[0x80 + 4 * 16 + 1] = 0x7FFF;
    log
}

#[test]
fn the_sprites_under_line_224_are_poses_of_their_own_found_by_their_exact_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let frame = overscan_log(40, 232);
    assert!(poses::sprites(&frame).is_empty(), "the capture kept no sprite under line 224");
    let low = poses::overscan_sprites_in(&frame, &poses::View::window());
    assert_eq!(low.len(), 1);
    let found = poses::overscan_effects(&frame, &low);
    assert_eq!(found.len(), 1);
    let pose = &found[0];
    assert_eq!((pose.w, pose.h, pose.org), (8, 8, (40, 233)));
    write_png(&dir.path().join("art/l.png"), 32, 32, |_, _| [0, 0, 255, 255]);
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(pose, "art/l.png", false)]),
    );
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&plain, 41 * 4 + 1, 234 * 4 + 1), [255, 255, 255]);
    assert_eq!(
        pixel(&drawn, 41 * 4 + 1, 234 * 4 + 1),
        [0, 0, 255],
        "the band is drawn from the art"
    );
    assert_eq!(pixel(&drawn, 5, 5), pixel(&plain, 5, 5));
    // Art for other pixels leaves the sprite as the game draws it
    let mut other = object(pose, "art/l.png", false);
    other.key ^= 1;
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![other]));
    renderer.set_art(Some(pack));
    assert_eq!(renderer.render_hd(&frame, options(4)).unwrap(), plain);
}

#[test]
fn a_field_figure_that_runs_under_line_224_is_drawn_whole_from_the_art_of_its_pieces_together() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/b.png"), 32, 72, |_, _| [0, 0, 255, 255]);
    // Two solid 8 x 8 pieces of one figure: one above line 224, one wholly under it (OAM y 224)
    let mut frame = overscan_log(40, 215);
    frame.oam_low[4..8].copy_from_slice(&[40, 224, 0, 4 << 1]);
    let sprites = poses::sprites(&frame);
    let band = poses::overscan_sprites_in(&frame, &poses::View::window());
    assert_eq!((sprites.len(), band.len()), (1, 1));
    let view = poses::View::window();
    let found = poses::poses(&frame, &sprites);
    assert_eq!(found.iter().filter(|pose| pose.field).count(), 1);
    // The art is made for the two pieces together
    let whole: Vec<&poses::Sprite> = sprites.iter().chain(band.iter()).collect();
    let whole = poses::field_pose(&whole, &frame, &view, 1).unwrap();
    let mut art = object(&whole, "art/b.png", false);
    art.id = "field-band-000".to_owned();
    let found =
        poses::join_band(found, &sprites, &band, &frame, &view, &mut |pose| pose.key == art.key);
    let joined: Vec<_> = found.iter().filter(|pose| pose.field).collect();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].sprites, 0b11, "the piece under the line is the figure's");
    // Without art for the whole, the pieces stay apart
    let found = poses::poses(&frame, &sprites);
    let found = poses::join_band(found, &sprites, &band, &frame, &view, &mut |_| false);
    assert_eq!(found.iter().find(|pose| pose.field).unwrap().sprites, 0b1);
    // The renderer draws both pieces from the art
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![art]));
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&plain, 41 * 4 + 1, 228 * 4 + 1), [255, 255, 255]);
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 228 * 4 + 1), [0, 0, 255]);
}

#[test]
fn an_effect_piece_under_line_224_is_placed_beside_the_band_pieces_a_figure_took() {
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/f.png"), 32, 72, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/e.png"), 32, 64, |_, _| [255, 0, 0, 255]);
    // A figure of palette 4 (a piece above line 224 and one under it) and a piece of palette 3
    // under it, within 4 pixels of the figure's: the effect cluster holds both lower pieces
    let mut frame = overscan_log(40, 215);
    frame.oam_low[4..8].copy_from_slice(&[40, 224, 0, 4 << 1]);
    frame.oam_low[8..12].copy_from_slice(&[40, 234, 0, 3 << 1]);
    frame.cgram_start[0x80 + 3 * 16 + 1] = 0x7FFF;
    let view = poses::View::window();
    let sprites = poses::sprites(&frame);
    let band = poses::overscan_sprites_in(&frame, &view);
    let whole: Vec<&poses::Sprite> = sprites.iter().chain(band.iter().take(1)).collect();
    let whole = poses::field_pose(&whole, &frame, &view, 1).unwrap();
    let mut figure = object(&whole, "art/f.png", false);
    figure.id = "field-band-001".to_owned();
    let effects = poses::overscan_effects(&frame, &band);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].sprites, 0b110, "the cluster holds the figure's lower piece");
    let effect = object(&effects[0], "art/e.png", false);
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![figure, effect]));
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 228 * 4 + 1), [0, 0, 255], "the figure's piece");
    assert_eq!(pixel(&plain, 41 * 4 + 1, 236 * 4 + 1), [255, 255, 255]);
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 236 * 4 + 1), [255, 0, 0], "the effect's own piece");
}

/// A pack of two portrait streams, each with one picture over its first 16 tiles (the keys of the
/// tiles are told apart by `seed`), and the frame whose BG1 shows a 4 x 4 block of characters at
/// map cell (2, 2): the tile of cell `n` is `tiles(n)`, a stream and a tile number.
fn portrait_block(
    dir: &std::path::Path,
    tiles: impl Fn(usize) -> (usize, usize),
) -> (Arc<Pack>, FrameLog) {
    write_png(&dir.join("art/a.png"), 128, 128, |_, _| [10, 0, 0, 255]);
    write_png(&dir.join("art/b.png"), 128, 128, |_, _| [0, 10, 0, 255]);
    let character_of = |stream: usize, tile: usize| {
        let mut chars = character();
        chars[1] = (stream * 100 + tile + 1) as u16;
        chars
    };
    let asset = |stream: usize, id: &str| Asset {
        id: id.to_owned(),
        bpp: 4,
        mode: Mode::Indexed,
        columns: 16,
        keys: (0..16).map(|tile| word_key(&character_of(stream, tile))).collect(),
        art: String::new(),
    };
    let assets = vec![asset(0, "portrait/16:8A9B/4bpp"), asset(1, "portrait/16:A780/4bpp")];
    let picture = |name: &str, asset: &str, art: &str| Picture {
        name: name.to_owned(),
        asset: asset.to_owned(),
        tiles: std::array::from_fn(|n| n as u16),
        palette: Palette::default(),
        art: art.to_owned(),
    };
    let art = ArtIndex::new(
        vec![
            picture("face-8A9B-1", "portrait/16:8A9B/4bpp", "art/a.png"),
            picture("face-A780-1", "portrait/16:A780/4bpp", "art/b.png"),
        ],
        Vec::new(),
    );
    let mut log = log(regs());
    for cell in 0..16_usize {
        let (stream, tile) = tiles(cell);
        // Character `cell + 1` of the base holds the tile
        let at = 0x1000 + (cell + 1) * 16;
        log.vram[at..at + 16].copy_from_slice(&character_of(stream, tile));
        log.vram[0x0400 + (2 + cell / 4) * 32 + 2 + cell % 4] = (cell + 1) as u16;
    }
    (pack_of(dir, assets, art), log)
}

#[test]
fn a_portrait_drawn_from_one_stream_is_whole() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, log) = portrait_block(dir.path(), |cell| (0, cell));
    let tally = tallied(pack, &log);
    assert_eq!(tally.portraits.len(), 1, "{:?}", tally.portraits);
    let portrait = &tally.portraits[0];
    assert_eq!(portrait.cells, 0xFFFF);
    assert_eq!(portrait.fault(), None);
}

#[test]
fn a_portrait_whose_halves_are_two_streams_is_drawn_whole_from_one() {
    let dir = tempfile::tempdir().unwrap();
    // The top two rows of one character, the bottom two of another: the game's VRAM mixed two faces
    let (pack, log) = portrait_block(dir.path(), |cell| (usize::from(cell >= 8), cell));
    let tally = tallied(pack, &log);
    assert_eq!(tally.portraits.len(), 1, "{:?}", tally.portraits);
    let portrait = &tally.portraits[0];
    assert_eq!(portrait.pictures.len(), 1, "one picture draws all of it: {portrait:?}");
    assert_eq!(portrait.cells, 0xFFFF, "every cell");
    assert_eq!(portrait.bound_assets.len(), 2, "the characters were of two streams");
    assert_eq!(portrait.rows.len(), 1);
    // (the audit still says what the game showed)
    let fault = portrait.fault().unwrap();
    assert!(fault.contains("characters are of") && fault.contains("16:A780"), "{fault}");
}

#[test]
fn a_portrait_whose_halves_use_two_palette_rows_is_drawn_in_one() {
    let dir = tempfile::tempdir().unwrap();
    let (pack, mut log) = portrait_block(dir.path(), |cell| (usize::from(cell >= 8), cell));
    // The top two rows are also in the palette of another row (BG palette 2)
    for cell in 0..8_usize {
        log.vram[0x0400 + (2 + cell / 4) * 32 + 2 + cell % 4] |= 2 << 10;
    }
    let tally = tallied(pack, &log);
    assert_eq!(tally.portraits.len(), 1, "{:?}", tally.portraits);
    let portrait = &tally.portraits[0];
    assert_eq!(portrait.pictures.len(), 1, "{portrait:?}");
    assert_eq!(portrait.rows.len(), 1, "one row: {portrait:?}");
}

/// Work RAM with the Kaioken on (`$0C36` bit 7) and Super Saiyan Goku in party record `slot`.
pub(super) fn wram_with_kaioken(on: bool, slot: usize) -> Vec<u8> {
    let mut wram = vec![0_u8; 0x20000];
    wram[0x0C36] = if on { 0x80 } else { 0x40 };
    wram[0x0B00 + 0x20 * slot + 1] = 0x3B;
    wram
}

#[test]
fn the_kaioken_condition_is_the_bit_of_the_state_and_goku_in_a_party_record() {
    let on = Conditions::of_work_ram(&wram_with_kaioken(true, 3));
    assert_eq!(
        on,
        Conditions {
            kaioken: true,
            super_saiyan_goku: true,
            kaioken_on: true,
            ending_flight: false,
            ssj_lightning: None
        }
    );
    let off = Conditions::of_work_ram(&wram_with_kaioken(false, 3));
    assert_eq!(
        off,
        Conditions {
            kaioken: false,
            super_saiyan_goku: true,
            kaioken_on: false,
            ending_flight: false,
            ssj_lightning: None
        },
        "Goku without it"
    );
    let mut no_goku = wram_with_kaioken(true, 3);
    no_goku[0x0B00 + 0x20 * 3 + 1] = 0x39;
    assert_eq!(
        Conditions::of_work_ram(&no_goku),
        Conditions {
            kaioken: false,
            super_saiyan_goku: false,
            kaioken_on: true,
            ending_flight: false,
            ssj_lightning: None
        },
        "the pack's Kaioken variants need Goku's unit $3B; the Kaioken itself is on (patch 0050)"
    );
    assert_eq!(Conditions::of_work_ram(&[]), Conditions::default(), "no work RAM says no");
    let mut multiplier = vec![0_u8; 0x20000];
    multiplier[0x01EF] = 5;
    assert_eq!(
        Conditions::of_work_ram(&multiplier),
        Conditions {
            kaioken: false,
            super_saiyan_goku: false,
            kaioken_on: true,
            ending_flight: false,
            ssj_lightning: None
        },
        "the Kaioken's multiplier in use (its first close-up, before $0C36 says it is on)"
    );
}

/// The multiplier (`$01EF`) is cleared only when the next turn starts (`05:83D8`), so it outlives the Kaioken: Goku turning
/// Super Saiyan in the turn he used it (Krillin falls to Frieza) has `$0C36` cleared by the trigger (`05:87B1`) and the
/// multiplier left. In the transformation (battle phase `$1A`, the unit still `$3A`) and after it (`$3B` in the party) it
/// is not the normal Goku using the Kaioken: his close-up and his powering up were drawn from the game's sprites in the
/// frames whose palette is not the Super Saiyan's.
#[test]
fn the_multiplier_left_by_the_kaioken_is_not_the_kaioken_while_goku_turns_super_saiyan() {
    let mut wram = vec![0_u8; 0x20000];
    wram[0x01EF] = 0x14;
    wram[0x00AB] = 0x04;
    wram[0x0B01] = 0x3A;
    assert!(
        Conditions::of_work_ram(&wram).kaioken_on,
        "the Kaioken's first close-up: the multiplier, $0C36 clear, the attack's animation (phase 4)"
    );
    wram[0x00AB] = 0x1A;
    assert!(
        !Conditions::of_work_ram(&wram).kaioken_on,
        "the transformation (phase $1A), the unit still $3A"
    );
    wram[0x00AB] = 0x01;
    wram[0x0B01] = 0x3B;
    assert!(
        !Conditions::of_work_ram(&wram).kaioken_on,
        "after it: $3B in the party, the multiplier not cleared before the next turn"
    );
    wram[0x0C36] = 0x80;
    assert!(
        Conditions::of_work_ram(&wram).kaioken_on,
        "the Kaioken's own bit still says it is on (a pack's Super Saiyan Kaioken)"
    );
}

#[test]
fn super_saiyan_goku_is_drawn_from_his_own_art_for_the_pixels_he_shares_with_the_normal_goku() {
    let dir = tempfile::tempdir().unwrap();
    // The 16 x 16 fighter of `fighter_log`: the normal Goku's art for its pixels is blue, Super Saiyan
    // Goku's red. The normal unit has a second pose (other pixels), so it is the unit with the most
    // poses: the one `unit_for` gave before the party's Super Saiyan Goku was taken into account.
    let (frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/normal.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/other.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    let goku = |id: &str, key: u64, art: &str| Object {
        id: id.to_owned(),
        key,
        ..object(&pose, art, false)
    };
    let objects = vec![
        goku("battle-goku-000", pose.key, "art/normal.png"),
        goku("battle-goku-001", pose.key ^ 1, "art/other.png"),
        goku("battle-goku-ssj-000", pose.key, "art/ssj.png"),
    ];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    // A pixel of the sprite
    let (x, y) = (41 * 4 + 1, 51 * 4 + 1);
    renderer.set_conditions(Conditions::default());
    let without = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&without, x, y), [0, 0, 255], "no Super Saiyan Goku: the normal unit");
    // Goku with the Kaioken off: `wram_with_kaioken(false, 0)` has his unit in the first party record
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let with = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&with, x, y), [255, 0, 0], "Super Saiyan Goku: his own art");
    // The close-ups of the special attacks are another set of Goku's art: its Super Saiyan set is drawn the same way
    let gallery = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(
            Vec::new(),
            vec![
                goku("gallery-party-goku-u00-000", pose.key, "art/normal.png"),
                goku("gallery-party-goku-u00-001", pose.key ^ 1, "art/other.png"),
                goku("gallery-party-goku-ssj-000", pose.key, "art/ssj.png"),
            ],
        ),
    );
    renderer.set_art(Some(gallery));
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let close_up = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&close_up, x, y), [255, 0, 0], "Super Saiyan Goku's close-up: his own art");
    // A pose that the Super Saiyan art has no picture of (its pixels are the other ones) stays the normal art
    let normal_only = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(
            Vec::new(),
            vec![
                goku("battle-goku-000", pose.key, "art/normal.png"),
                goku("battle-goku-001", pose.key ^ 1, "art/other.png"),
                goku("battle-goku-ssj-001", pose.key ^ 1, "art/ssj.png"),
            ],
        ),
    );
    renderer.set_art(Some(normal_only));
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let kept = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&kept, x, y), [0, 0, 255], "no Super Saiyan art for the pose: the normal one");
    // A Super Saiyan set with more poses than the normal one is still not drawn for the normal Goku
    let bigger = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(
            Vec::new(),
            vec![
                goku("battle-goku-000", pose.key, "art/normal.png"),
                goku("battle-goku-ssj-000", pose.key, "art/ssj.png"),
                goku("battle-goku-ssj-001", pose.key ^ 1, "art/ssj.png"),
                goku("battle-goku-ssj-002", pose.key ^ 2, "art/ssj.png"),
            ],
        ),
    );
    renderer.set_art(Some(bigger));
    renderer.set_conditions(Conditions::default());
    let normal = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&normal, x, y), [0, 0, 255], "the normal Goku: the normal art");
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let ssj = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&ssj, x, y), [255, 0, 0], "Super Saiyan Goku: his own art");
}

/// A pack of Goku's normal and Super Saiyan art for the pixels of the fighter of [`fighter_log`] (palette 1, colour 2
/// green): `battle-goku-000` (blue) and `battle-goku-001` (other pixels), so the normal unit has the most poses, and
/// `battle-goku-ssj-000` (red), each palette the colours given by index. (The pack of the game: `battle-goku-017` and
/// `battle-goku-ssj-021` have the same key, the normal palette has black hair at 14 and 15, the Super Saiyan one yellow.)
fn goku_pack(
    dir: &tempfile::TempDir,
    pose: &Pose,
    normal: &[(usize, [u8; 3])],
    super_saiyan: &[(usize, [u8; 3])],
) -> Arc<Pack> {
    write_png(&dir.path().join("art/normal.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/other.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    let unit = |id: &str, key: u64, art: &str, colours: &[(usize, [u8; 3])]| {
        let mut held = Object { id: id.to_owned(), key, ..object(pose, art, false) };
        for &(index, colour) in colours {
            held.palette.mask |= 1 << index;
            held.palette.colors[index] = colour;
        }
        held
    };
    let objects = vec![
        unit("battle-goku-000", pose.key, "art/normal.png", normal),
        unit("battle-goku-001", pose.key ^ 1, "art/other.png", normal),
        unit("battle-goku-ssj-000", pose.key, "art/ssj.png", super_saiyan),
    ];
    pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects))
}

/// The object the fighter of [`fighter_log`] is drawn from, under `conditions`.
fn goku_drawn(pack: Arc<Pack>, frame: &FrameLog, conditions: Conditions) -> Option<u32> {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.set_conditions(conditions);
    renderer.render_hd(frame, options(4)).unwrap();
    renderer.art_stats().poses.iter().find(|p| p.fighter).unwrap().art
}

#[test]
fn super_saiyan_goku_in_the_party_coloured_as_the_normal_goku_is_drawn_from_the_normal_art() {
    // The ending: unit `$3B` is still in the party, and the frame draws Goku in the normal palette (colour 2, his
    // hair, is the frame's green in the normal art, yellow in the Super Saiyan art; colour 3 is the same in both)
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let pack = goku_pack(
        &dir,
        &pose,
        &[(2, [0, 255, 0]), (3, [0, 0, 0])],
        &[(2, [255, 198, 0]), (3, [0, 0, 0])],
    );
    let drawn = goku_drawn(pack, &frame, Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    assert_eq!(drawn, Some(0), "battle-goku-000: the normal Goku's colours, the normal art");
}

#[test]
fn super_saiyan_goku_in_the_party_coloured_as_the_super_saiyan_is_drawn_from_the_super_saiyan_art()
{
    // A fight: the frame draws Goku in the Super Saiyan palette (colour 2 the frame's green in the Super Saiyan art,
    // black in the normal art)
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let pack = goku_pack(
        &dir,
        &pose,
        &[(2, [0, 0, 0]), (3, [0, 0, 0])],
        &[(2, [0, 255, 0]), (3, [0, 0, 0])],
    );
    let ssj = Conditions::of_work_ram(&wram_with_kaioken(false, 0));
    assert_eq!(
        goku_drawn(Arc::clone(&pack), &frame, ssj),
        Some(2),
        "battle-goku-ssj-000: the Super Saiyan's colours"
    );
    // Without his unit in the party the colours decide too: strictly nearer the Super Saiyan art's, his Super Saiyan
    // art (he turns Super Saiyan before Frieza before `$3B` is in a party record)
    assert_eq!(
        goku_drawn(pack, &frame, Conditions::default()),
        Some(2),
        "no $3B, the Super Saiyan's colours: the Super Saiyan art"
    );
    // and in the normal Goku's colours, the normal art
    let normal = goku_pack(
        &dir,
        &pose,
        &[(2, [0, 255, 0]), (3, [0, 0, 0])],
        &[(2, [0, 0, 0]), (3, [0, 0, 0])],
    );
    assert_eq!(
        goku_drawn(normal, &frame, Conditions::default()),
        Some(0),
        "no $3B, the normal colours: the normal art"
    );
}

#[test]
fn goku_turning_super_saiyan_before_frieza_before_3b_is_in_the_party_is_drawn_from_the_super_saiyan_art()
 {
    // secret-ssj-frieza from frame 6188: the close-up `battle-goku-068` (and `battle-goku-ssj-070`, the same key) in
    // the Super Saiyan palette (13 blue, 14 pale yellow, 15 gold) while no party record has `$3B` yet. The normal art
    // has 13, 14 and 15 black: it drew him with black hair. The palette decides: the Super Saiyan art. Colours as near
    // to both leave it to the party record: the normal art
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let (blue, yellow, gold) =
        (poses::rgb555(0x7C00), poses::rgb555(0x53FF), poses::rgb555(0x0A98));
    let black = [0, 0, 0];
    let pack = goku_pack(
        &dir,
        &pose,
        &[(1, black), (2, [0, 255, 0]), (13, black), (14, black), (15, black)],
        &[(1, black), (2, [0, 255, 0]), (13, blue), (14, yellow), (15, gold)],
    );
    for (index, colour) in [(13, 0x7C00), (14, 0x53FF), (15, 0x0A98)] {
        frame.cgram_start[0x80 + 16 + index] = colour;
    }
    let drawn = goku_drawn(Arc::clone(&pack), &frame, Conditions::default());
    assert_eq!(drawn, Some(2), "the Super Saiyan palette, no $3B: battle-goku-ssj-000");
    // The frame before (6186): the normal palette, black hair: the normal art
    for index in [13, 14, 15] {
        frame.cgram_start[0x80 + 16 + index] = 0;
    }
    let drawn = goku_drawn(Arc::clone(&pack), &frame, Conditions::default());
    assert_eq!(drawn, Some(0), "the normal palette: battle-goku-000");
    // As near to both (two sets whose palettes are alike): the normal art without `$3B`
    frame.cgram_start[0x80 + 16 + 13] = 0x7C00;
    frame.cgram_start[0x80 + 16 + 14] = 0x53FF;
    frame.cgram_start[0x80 + 16 + 15] = 0x0A98;
    let tie = goku_pack(
        &dir,
        &pose,
        &[(1, black), (2, [0, 255, 0]), (13, blue), (14, yellow), (15, black)],
        &[(1, black), (2, [0, 255, 0]), (13, blue), (14, yellow), (15, [0, 0, 0])],
    );
    assert_eq!(goku_drawn(tie, &frame, Conditions::default()), Some(0), "as near: the normal art");
}

#[test]
fn a_colour_only_one_of_goku_s_palettes_has_does_not_decide_between_his_two_sets() {
    // Both sets have the frame's green at colour 2; only the Super Saiyan palette has colour 4 (a yellow far from the
    // frame's black). Compared over the colours both use the two are as near: the party record decides, the Super
    // Saiyan art. (Summed over each palette's own colours the normal art would be nearer.)
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let pack =
        goku_pack(&dir, &pose, &[(2, [0, 255, 0])], &[(2, [0, 255, 0]), (4, [255, 222, 41])]);
    let drawn = goku_drawn(pack, &frame, Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    assert_eq!(drawn, Some(2), "battle-goku-ssj-000: as near, the party record");
}

#[test]
fn goku_holding_gohan_before_super_vegeta_is_drawn_from_the_super_saiyan_art_whatever_its_colours()
{
    // The ending's colours (colour 2 the row's green in the normal art, yellow in the Super Saiyan art) on the pose of
    // Goku holding Gohan (`battle-goku-017`, `battle-goku-ssj-021`): the Super Saiyan art while `$3B` is in the party,
    // a choice of this branch. The same colours on another pose give the normal art, and so does this pose without him.
    let dir = tempfile::tempdir().unwrap();
    let (_, pose) = fighter_log(40, 50);
    let mut row = [[0_u8; 3]; 16];
    row[2] = [0, 255, 0];
    let unit_for = |key: u64, conditions: Conditions| {
        let pose = Pose { key, ..pose.clone() };
        let pack = goku_pack(&dir, &pose, &[(2, [0, 255, 0])], &[(2, [255, 198, 0])]);
        let mut art = ArtState::new(pack);
        art.conditions = conditions;
        art.unit_for(&pose, &row)
    };
    let ssj = Conditions::of_work_ram(&wram_with_kaioken(false, 0));
    assert_eq!(
        unit_for(super::GOKU_HOLDING_GOHAN, ssj).as_deref(),
        Some("battle-goku-ssj"),
        "the Super Saiyan art for this pose"
    );
    assert_eq!(
        unit_for(super::GOKU_HOLDING_GOHAN ^ 1, ssj).as_deref(),
        Some("battle-goku"),
        "another pose"
    );
    assert_eq!(
        unit_for(super::GOKU_HOLDING_GOHAN, Conditions::default()).as_deref(),
        Some("battle-goku"),
        "no $3B in the party"
    );
}

/// A pack of two units with art for the pixels of the fighter of [`fighter_log`] (palette 1, colour 2
/// green): `battle-vegeta`, blue art, with a second pose (other pixels), so the unit with the most
/// poses, and `battle-super-vegeta`, red art, of one pose; colour 2 of each palette as given. (The
/// pack of the game: `battle-vegeta-034` and `battle-super-vegeta-000` have the same source key, the
/// normal palette has black at 1, 14 and 15, the Super one yellow and orange at 14 and 15.)
fn vegeta_pack(
    dir: &tempfile::TempDir,
    pose: &Pose,
    vegeta: [u8; 3],
    super_vegeta: [u8; 3],
) -> Arc<Pack> {
    write_png(&dir.path().join("art/vegeta.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/super.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/other.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    let unit = |id: &str, key: u64, art: &str, colour: [u8; 3]| {
        let mut held = Object { id: id.to_owned(), key, ..object(pose, art, false) };
        held.palette = Palette { mask: 0b100, ..Palette::default() };
        held.palette.colors[2] = colour;
        held
    };
    let objects = vec![
        unit("battle-vegeta-034", pose.key, "art/vegeta.png", vegeta),
        unit("battle-vegeta-035", pose.key ^ 1, "art/other.png", vegeta),
        unit("battle-super-vegeta-000", pose.key, "art/super.png", super_vegeta),
    ];
    pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects))
}

#[test]
fn a_pose_two_units_have_art_for_is_drawn_from_the_unit_made_for_its_exact_colours() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    // The game draws the pose in green: the Super unit's art is made for it, the normal unit's
    // (more poses) for black, which recolouring cannot turn into the Super one's colours
    let pack = vegeta_pack(&dir, &pose, [0, 0, 0], [0, 255, 0]);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [255, 0, 0], "the unit of the exact colours");
    let stats = renderer.art_stats();
    let fighter = stats.poses.iter().find(|p| p.fighter).unwrap();
    assert_eq!(fighter.art, Some(2), "battle-super-vegeta-000");
    assert!(!fighter.recoloured, "drawn in its own colours");
}

#[test]
fn a_pose_no_unit_has_art_for_its_exact_colours_is_drawn_from_the_unit_with_the_most_poses() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    // Neither palette is the game's green (the Super one is one step of a channel off): the unit
    // with the most poses, as before
    let pack = vegeta_pack(&dir, &pose, [0, 0, 0], [0, 254, 0]);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let fighter = stats.poses.iter().find(|p| p.fighter).unwrap();
    // (both arts are recoloured to green here, so the drawn colour does not tell them apart)
    assert_eq!(fighter.art, Some(0), "battle-vegeta-034: the unit with the most poses");
    assert!(fighter.recoloured);
}

/// Vegeta's palette as the pack's art of his poses has it: the outline (1) and the hair (14, 15) black, colour 2 the
/// green of the fighter of [`fighter_log`]; with `hair`, Super Vegeta's (yellow, orange).
fn vegeta_palette(hair: Option<([u8; 3], [u8; 3])>) -> Palette {
    let mut palette = Palette { mask: 1 << 1 | 1 << 2 | 1 << 14 | 1 << 15, ..Palette::default() };
    palette.colors[2] = [0, 255, 0];
    if let Some((yellow, orange)) = hair {
        palette.colors[14] = yellow;
        palette.colors[15] = orange;
    }
    palette
}

#[test]
fn super_vegeta_drawn_with_the_tiles_of_a_pose_only_vegeta_has_art_for_keeps_the_game_s_sprite() {
    // The game draws Super Vegeta's energy wave with Vegeta's tiles in Super Vegeta's palette (yellow and orange hair
    // at 14 and 15); the pack has art of that pose for Vegeta only (`gallery-enemy-vegeta-u15-087`), black hair, the
    // colour of his outline. Recoloured, it drew him with black hair: the pose keeps the game's own sprite
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/vegeta.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let vegeta = Object {
        id: "gallery-enemy-vegeta-u15-087".to_owned(),
        palette: vegeta_palette(None),
        ..object(&pose, "art/vegeta.png", false)
    };
    // His palette is known from the art of his other poses (another key, another size)
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: pose.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        width: 8,
        height: 8,
        ..object(&pose, "art/vegeta.png", false)
    };
    // and a piece of him with no hair (his arm: the outline and colour 2), which says nothing of the hair
    let mut arm = Object {
        id: "battle-super-vegeta-001".to_owned(),
        key: pose.key ^ 2,
        ..super_vegeta.clone()
    };
    arm.palette.mask = 1 << 1 | 1 << 2;
    let objects = vec![vegeta, super_vegeta, arm];
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    // Vegeta's own colours: his art
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "Vegeta: the art");
    assert_eq!(renderer.art_stats().poses.iter().find(|p| p.fighter).unwrap().art, Some(0));
    // Super Vegeta's: the sprite, in the frame's green
    frame.cgram_start[0x80 + 16 + 14] = 0x03FF; // yellow
    frame.cgram_start[0x80 + 16 + 15] = 0x11DF; // orange
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 255, 0], "Super Vegeta: the sprite");
    let stats = renderer.art_stats();
    let fighter = stats.poses.iter().find(|p| p.fighter).unwrap();
    assert_eq!((fighter.art, fighter.matched), (None, false), "not Vegeta's art");
    // Two other colours of hair, not Super Vegeta's palette: recoloured, as before
    frame.cgram_start[0x80 + 16 + 15] = 0x7C00; // blue
    renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let fighter = stats.poses.iter().find(|p| p.fighter).unwrap();
    assert_eq!((fighter.art, fighter.recoloured), (Some(0), true), "not his palette: the art");
    // The whole palette a few steps darker (a fade) keeps his three blacks one colour: still his art
    for index in [1, 14, 15] {
        frame.cgram_start[0x80 + 16 + index] = 0x0421;
    }
    renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    assert_eq!(stats.poses.iter().find(|p| p.fighter).unwrap().art, Some(0), "a fade: the art");
}

#[test]
fn super_saiyan_goku_in_a_pose_only_normal_art_exists_for_keeps_the_game_s_sprite() {
    // Super Saiyan Goku's palette (13 blue, 14 and 15 pale yellow and gold) on tiles the pack has normal art for only,
    // outline and hair black (a piece of his head at the screen's edge in the hidden fight, `noartfix-f884cde40d`):
    // recoloured, it drew him with black hair. With Super Saiyan art for the pose the party record decides between
    // the sets, as before
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/goku.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let (blue, yellow, gold) =
        (poses::rgb555(0x7C00), poses::rgb555(0x53FF), poses::rgb555(0x0A98));
    let goku = |id: &str, key: u64, art: &str, hair: Option<[[u8; 3]; 3]>| {
        let mut palette =
            Palette { mask: 1 << 1 | 1 << 2 | 1 << 13 | 1 << 14 | 1 << 15, ..Palette::default() };
        palette.colors[2] = [0, 255, 0];
        if let Some([thirteen, fourteen, fifteen]) = hair {
            palette.colors[13] = thirteen;
            palette.colors[14] = fourteen;
            palette.colors[15] = fifteen;
        }
        Object { id: id.to_owned(), key, palette, ..object(&pose, art, false) }
    };
    let normal = goku("battle-goku-000", pose.key, "art/goku.png", None);
    // his Super Saiyan palette, known from the art of another pose (another key, another size)
    let other = Object {
        width: 8,
        height: 8,
        ..goku("battle-goku-ssj-019", pose.key ^ 1, "art/ssj.png", Some([blue, yellow, gold]))
    };
    for (index, colour) in [(13, 0x7C00), (14, 0x53FF), (15, 0x0A98)] {
        frame.cgram_start[0x80 + 16 + index] = colour;
    }
    let drawn_with = |objects: Vec<Object>| {
        let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        let art = renderer.art_stats().poses.iter().find(|p| p.fighter).unwrap().art;
        (pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), art)
    };
    assert_eq!(
        drawn_with(vec![normal.clone(), other.clone()]),
        ([0, 255, 0], None),
        "only the normal art: the game's sprite"
    );
    // The Super Saiyan set has art for the pose: `unit_for` decides between the sets, here (no `$3B`, his colours) the
    // Super Saiyan art
    let own = goku("battle-goku-ssj-021", pose.key, "art/ssj.png", Some([blue, yellow, gold]));
    let (_, art) = drawn_with(vec![normal, other, own]);
    assert_eq!(art, Some(2), "a Super Saiyan art for the pose: unit_for decides");
}

#[test]
fn the_sprites_super_vegeta_s_pose_keeps_are_not_drawn_by_the_art_of_a_field_group() {
    // The same sprites are a field group too (a 71 x 55 dash of Super Vegeta in the hidden fight is also the key of
    // `field-frieza-force-000`): once Vegeta's art is not drawn for the fighter, the group must not take its sprites
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let group = poses::poses(&frame, &poses::sprites(&frame))
        .into_iter()
        .find(|pose| pose.field)
        .expect("the field group of the same sprites");
    write_png(&dir.path().join("art/vegeta.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/field.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let vegeta = Object {
        id: "gallery-party-vegeta-u80-070".to_owned(),
        palette: vegeta_palette(None),
        ..object(&pose, "art/vegeta.png", false)
    };
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: pose.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        width: 8,
        height: 8,
        ..object(&pose, "art/vegeta.png", false)
    };
    let field = Object {
        id: "field-frieza-force-000".to_owned(),
        key: group.key,
        palette: vegeta_palette(Some(([255, 0, 0], [160, 0, 0]))),
        width: group.w as u16,
        height: group.h as u16,
        ..object(&pose, "art/field.png", false)
    };
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![vegeta, super_vegeta, field]),
    );
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    frame.cgram_start[0x80 + 16 + 14] = 0x03FF;
    frame.cgram_start[0x80 + 16 + 15] = 0x11DF;
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 255, 0], "the game's sprite");
    let stats = renderer.art_stats();
    assert!(stats.poses.iter().all(|p| !p.matched), "no art for the fighter nor the group");
}

#[test]
fn vegeta_s_art_found_by_its_shape_that_would_lose_a_colour_of_super_vegeta_s_palette_is_not_drawn()
{
    // Vegeta's art of the same silhouette (another key): made for black at 1 and 14, while the sprite's colour 1 is
    // yellow and its 14 black, a palette of Super Vegeta's art. Recolouring would give both one colour: the sprite
    // stays
    let dir = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/b.png"), 64, 60, |_, _| [0, 0, 0, 255]);
    let (frame, pose) = banded_log(40, 50, [0x03FF, 0x7FE0, 0x7C1F]);
    let shape = |fourteen: [u8; 3]| {
        let mut shape = object(&pose, "art/b.png", false);
        shape.id = "gallery-enemy-vegeta-u15-056".to_owned();
        shape.key ^= 1;
        shape.mask = vec![0; pose.arr.len().div_ceil(8)];
        for n in (0..pose.arr.len()).filter(|&n| pose.arr[n] != 0) {
            shape.mask[n / 8] |= 1 << (n % 8);
        }
        shape.palette = Palette { mask: 0b1110 | 1 << 14, ..Palette::default() };
        shape.palette.colors[2] = [255, 255, 255];
        shape.palette.colors[3] = [128, 128, 128];
        shape.palette.colors[14] = fourteen;
        shape
    };
    // Super Vegeta's palette, the sprite's colours (another key, another size)
    let mut super_vegeta = object(&pose, "art/b.png", false);
    super_vegeta.id = "battle-super-vegeta-000".to_owned();
    super_vegeta.key ^= 2;
    (super_vegeta.width, super_vegeta.height) = (8, 8);
    super_vegeta.palette = Palette { mask: 0b1110 | 1 << 14, ..Palette::default() };
    for (index, colour) in [(1, 0x03FF), (2, 0x7FE0), (3, 0x7C1F), (14, 0)] {
        super_vegeta.palette.colors[index] = poses::rgb555(colour);
    }
    // and a field group's art of the same silhouette, which the sprites the pose keeps are not given to
    let mut field = shape([90, 0, 0]);
    field.id = "field-x-000".to_owned();
    field.key ^= 3;
    let drawn_from = |object: Object| {
        let objects = vec![object, super_vegeta.clone(), field.clone()];
        let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        renderer.render_hd(&frame, options(4)).unwrap();
        let stats = renderer.art_stats();
        let others = stats.poses[1..].iter().filter(|p| p.matched).count();
        (stats.poses[0].matched, stats.poses[0].recoloured, others)
    };
    assert_eq!(drawn_from(shape([0, 0, 0])), (false, false, 0), "black at 1 and 14: the sprite");
    assert_eq!(drawn_from(shape([90, 0, 0])), (true, true, 0), "another colour at 14: recoloured");
}

/// A pack with the picture `art/o.png` of the fighter of [`fighter_log`] (blue) and the pose variant `art/v.png` (red, `size`
/// art pixels) of the same object, drawn while the Kaioken is on; the variant's margin is 2 game pixels.
pub(super) fn variant_pack(
    dir: &tempfile::TempDir,
    pose: &Pose,
    variant: Option<(u32, u32)>,
) -> Arc<Pack> {
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let mut pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![object(pose, "art/o.png", false)]),
    );
    if let Some((width, height)) = variant {
        write_png(&dir.path().join("art/v.png"), width, height, |_, _| [255, 0, 0, 255]);
        let file = dir.path().join("art/v.png").to_string_lossy().into_owned();
        let variants = vec![PoseVariant {
            art: "battle-x-000".to_owned(),
            when: ssd_mod::pose::Condition::Kaioken,
            margin: 2,
            file,
        }];
        Arc::get_mut(&mut pack).unwrap().variants = variants;
    }
    pack
}

#[test]
fn a_pose_variant_is_drawn_over_its_box_and_its_margin_while_the_kaioken_is_on() {
    let dir = tempfile::tempdir().unwrap();
    // The 16 x 16 fighter at (40, 50), the variant 4 game pixels larger (2 on each side): 80 x 80 art pixels
    let (frame, pose) = fighter_log(40, 50);
    let pack = variant_pack(&dir, &pose, Some((80, 80)));
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let red = [255, 0, 0];
    // Off: the object's own picture, and nothing past the sprite
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let off = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&off, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "the object's picture");
    assert_eq!(pixel(&off, 38 * 4 + 1, 48 * 4 + 1), pixel(&plain, 38 * 4 + 1, 48 * 4 + 1));
    // On: the variant covers the sprite and its margin (frame 38 to 57, 48 to 67), and nothing past it
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(true, 0)));
    let on = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&on, 41 * 4 + 1, 51 * 4 + 1), red, "the sprite's pixels");
    assert_eq!(pixel(&on, 38 * 4, 48 * 4), red, "the margin's first pixel");
    assert_eq!(pixel(&on, 57 * 4 + 3, 67 * 4 + 3), red, "the margin's last pixel");
    assert_ne!(pixel(&on, 37 * 4 + 3, 50 * 4), red, "nothing past the margin on the left");
    assert_ne!(pixel(&on, 40 * 4, 47 * 4 + 3), red, "nor above it");
    // Away from the pose nothing changes
    assert_eq!(pixel(&on, 5, 5), pixel(&plain, 5, 5));
}

#[test]
fn a_pose_variant_that_is_missing_or_of_another_size_is_not_drawn_and_the_object_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let on = Conditions::of_work_ram(&wram_with_kaioken(true, 0));
    // The variant's picture must be the object's (16 + 2 x 2 game pixels, four art pixels to one): 80 x 80
    for variant in [Some((64, 64)), None] {
        let pack = variant_pack(&dir, &pose, variant);
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        renderer.set_conditions(on);
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 0, 255], "{variant:?}");
        assert_ne!(pixel(&drawn, 38 * 4, 48 * 4), [255, 0, 0], "{variant:?}");
    }
}

/// A pose variant of the object `art` for `when`, margin 2 game pixels, for the 16 x 16 fighter of [`fighter_log`]: a picture
/// of 80 x 80 art pixels of `rgb`, written to `dir` as `<name>.png`.
fn flat_variant(
    dir: &tempfile::TempDir,
    name: &str,
    art: &str,
    when: ssd_mod::pose::Condition,
    rgb: [u8; 3],
) -> PoseVariant {
    let path = dir.path().join(format!("art/{name}.png"));
    write_png(&path, 80, 80, |_, _| [rgb[0], rgb[1], rgb[2], 255]);
    PoseVariant { art: art.to_owned(), when, margin: 2, file: path.to_string_lossy().into_owned() }
}

#[test]
fn an_always_variant_is_drawn_over_its_box_and_its_margin_in_every_frame() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let mut pack = variant_pack(&dir, &pose, None);
    let always = ssd_mod::pose::Condition::Always;
    Arc::get_mut(&mut pack).unwrap().variants =
        vec![flat_variant(&dir, "always", "battle-x-000", always, [255, 0, 0])];
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(pack));
    let red = [255, 0, 0];
    // No condition of the game holds, and none is needed
    for conditions in [Conditions::default(), Conditions::of_work_ram(&wram_with_kaioken(true, 0))]
    {
        renderer.set_conditions(conditions);
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), red, "the sprite's pixels");
        assert_eq!(pixel(&drawn, 38 * 4, 48 * 4), red, "the margin's first pixel");
        assert_eq!(pixel(&drawn, 57 * 4 + 3, 67 * 4 + 3), red, "the margin's last pixel");
        assert_ne!(pixel(&drawn, 37 * 4 + 3, 50 * 4), red, "nothing past the margin on the left");
        assert_ne!(pixel(&drawn, 40 * 4, 47 * 4 + 3), red, "nor above it");
        assert_eq!(pixel(&drawn, 5, 5), pixel(&plain, 5, 5), "away from the pose nothing changes");
    }
}

#[test]
fn the_kaioken_variant_of_an_object_is_drawn_while_it_holds_and_its_always_variant_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let mut pack = variant_pack(&dir, &pose, None);
    let (always, kaioken) = (ssd_mod::pose::Condition::Always, ssd_mod::pose::Condition::Kaioken);
    // The `always` variant first: the order of the variants does not decide
    Arc::get_mut(&mut pack).unwrap().variants = vec![
        flat_variant(&dir, "always", "battle-x-000", always, [255, 0, 0]),
        flat_variant(&dir, "kaioken", "battle-x-000", kaioken, [255, 255, 0]),
    ];
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let off = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&off, 38 * 4, 48 * 4), [255, 0, 0], "the Kaioken off: `always`");
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(true, 0)));
    let on = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&on, 38 * 4, 48 * 4), [255, 255, 0], "the Kaioken on: `kaioken`");
}

/// The game's text font of a cartridge image that has only it: every tile of the UI set its own, and the space (tile `$01`)
/// colour 1 everywhere, as the font's own tests make it.
fn space_font() -> Arc<crate::font::Font> {
    // (the UI set is 384 tiles of 2 bits)
    let mut rom = vec![0_u8; crate::font::FONT_OFFSET + 384 * 16];
    let at = |code: usize| crate::font::FONT_OFFSET + code * 16;
    for code in 0..384 {
        rom[at(code) + 15] = (code % 250) as u8 + 1;
    }
    for row in 0..8 {
        rom[at(0x01) + 2 * row] = 0xFF;
    }
    rom[at(0x01) + 15] = 0;
    Arc::new(crate::font::Font::standard(&rom).unwrap())
}

/// The fighter of [`fighter_log`] at (40, 50) in front of a BG3 of the font's space in white on every cell (the white of a
/// Super Saiyan aura, or of a blast, behind the fighter): of priority 1 with BG3 over every layer when `window` (a window
/// in front of the fighter), of priority 0 otherwise (behind the fighter's priority-0 sprite).
fn fighter_on_spaces(window: bool) -> FrameLog {
    let (mut frame, _) = fighter_log(40, 50);
    for entry in &mut frame.entries {
        entry.regs.main_bg[2] = true;
        entry.regs.bg_map_base[2] = 0x0800;
        entry.regs.bg_char_base[2] = 0x2000;
        entry.regs.mode_1_bg3_priority = window;
    }
    // Tile 1 of BG3's characters: colour 1 on every pixel (the font's space)
    for row in 0..8 {
        frame.vram[0x2000 + 8 + row] = 0x00FF;
    }
    let entry = 1 | u16::from(window) << 13;
    frame.vram[0x0800..0x0C00].fill(entry);
    frame.cgram_start[1] = 0x7FFF;
    frame
}

#[test]
fn a_variants_margin_over_blank_text_cells_behind_the_fighter_is_not_painted_over_by_the_text() {
    // Super Saiyan Goku's raised hair (`battle-goku-ssj-020` in the aura of the transformation) was cut along the cells of
    // the aura's white BG3 spaces: the HD text filled every cell's pixels with its ground after the art
    let dir = tempfile::tempdir().unwrap();
    let (_, pose) = fighter_log(40, 50);
    let mut pack = variant_pack(&dir, &pose, None);
    let always = ssd_mod::pose::Condition::Always;
    Arc::get_mut(&mut pack).unwrap().variants =
        vec![flat_variant(&dir, "always", "battle-x-000", always, [255, 0, 0])];
    let red = [255, 0, 0];
    let frame = fighter_on_spaces(false);
    // Enhanced (the 256-pixel picture)
    let mut renderer = Renderer::new();
    renderer.set_font(Some(space_font()));
    renderer.set_art(Some(Arc::clone(&pack)));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), red, "the sprite's pixels");
    assert_eq!(pixel(&drawn, 38 * 4, 48 * 4), red, "the margin's first pixel, on a space");
    assert_eq!(pixel(&drawn, 57 * 4 + 3, 67 * 4 + 3), red, "the margin's last pixel, on a space");
    assert_eq!(pixel(&drawn, 45 * 4 + 2, 48 * 4 + 1), red, "the margin above the sprite");
    assert_eq!(
        pixel(&drawn, 37 * 4 + 3, 50 * 4),
        [0xFF; 3],
        "past the margin the cells keep their white"
    );
    // The wide view of a battle (the window 58 pixels in)
    let mut wide = WideRenderer::new();
    wide.set_font(Some(space_font()));
    wide.set_art(Some(pack));
    let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
    let shown = wide
        .render_hd(Rom(&[]), 0x02, None, &frame, options_wide, super::tests::options(4))
        .unwrap();
    let at = |x: usize, y: usize| pixel(&shown.picture, (58 + x) * 4, y * 4);
    assert_eq!(at(38, 48), red, "wide: the margin's first pixel, on a space");
    assert_eq!(at(45, 48), red, "wide: the margin above the sprite");
    assert_eq!(at(57, 67), red, "wide: the margin's last pixel");
    assert_eq!(at(37, 50), [0xFF; 3], "wide: past the margin the cells keep their white");
}

#[test]
fn a_variants_margin_under_a_text_window_in_front_of_the_fighter_is_still_covered_by_the_window() {
    // BG3 of priority 1 over every layer (a window in front of the fighter): the window's cells are drawn over the variant
    let dir = tempfile::tempdir().unwrap();
    let (_, pose) = fighter_log(40, 50);
    let mut pack = variant_pack(&dir, &pose, None);
    let always = ssd_mod::pose::Condition::Always;
    Arc::get_mut(&mut pack).unwrap().variants =
        vec![flat_variant(&dir, "always", "battle-x-000", always, [255, 0, 0])];
    let frame = fighter_on_spaces(true);
    let mut renderer = Renderer::new();
    renderer.set_font(Some(space_font()));
    renderer.set_art(Some(Arc::clone(&pack)));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&drawn, 38 * 4, 48 * 4), [0xFF; 3], "the margin under the window: the window");
    assert_eq!(pixel(&drawn, 45 * 4 + 2, 48 * 4 + 1), [0xFF; 3], "above the sprite too");
    let mut wide = WideRenderer::new();
    wide.set_font(Some(space_font()));
    wide.set_art(Some(pack));
    let options_wide = WideOptions { width: 372, hud: HudPlacement::Window };
    let shown = wide
        .render_hd(Rom(&[]), 0x02, None, &frame, options_wide, super::tests::options(4))
        .unwrap();
    assert_eq!(pixel(&shown.picture, (58 + 38) * 4, 48 * 4), [0xFF; 3], "wide: the window");
}

#[test]
fn an_always_variant_of_super_saiyan_goku_is_drawn_for_the_pixels_he_shares_only_while_he_is_in_the_party()
 {
    let dir = tempfile::tempdir().unwrap();
    // As in the test of the shared pixels above: the normal Goku's art is blue and has the most poses, Super Saiyan Goku's
    // red; the variant of `battle-goku-ssj-000` is yellow.
    let (frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/normal.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/other.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    let goku = |id: &str, key: u64, art: &str| Object {
        id: id.to_owned(),
        key,
        ..object(&pose, art, false)
    };
    let objects = vec![
        goku("battle-goku-000", pose.key, "art/normal.png"),
        goku("battle-goku-001", pose.key ^ 1, "art/other.png"),
        goku("battle-goku-ssj-000", pose.key, "art/ssj.png"),
    ];
    let mut pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    let always = ssd_mod::pose::Condition::Always;
    Arc::get_mut(&mut pack).unwrap().variants =
        vec![flat_variant(&dir, "hair", "battle-goku-ssj-000", always, [255, 255, 0])];
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let (x, y) = (41 * 4 + 1, 51 * 4 + 1);
    // The normal Goku (before the transformation): his own art, and no margin
    renderer.set_conditions(Conditions::default());
    let normal = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&normal, x, y), [0, 0, 255], "the normal Goku: the normal art");
    assert_ne!(pixel(&normal, 38 * 4, 48 * 4), [255, 255, 0], "no variant past the sprite");
    // Super Saiyan Goku in the party: the pose is drawn from his art, so his variant is drawn, margin included
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let ssj = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&ssj, x, y), [255, 255, 0], "Super Saiyan Goku: the variant");
    assert_eq!(pixel(&ssj, 38 * 4, 48 * 4), [255, 255, 0], "and its margin");
}

#[test]
fn the_packs_own_variant_is_drawn_with_its_margin_in_every_frame_and_a_pack_without_one_is_unchanged()
 {
    use ssd_mod::pose::Condition::{Always, Kaioken};
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let red = [255, 0, 0];
    let mut pack = variant_pack(&dir, &pose, None);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::new((*pack).clone())));
    let without = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(
        pixel(&without, 41 * 4 + 1, 51 * 4 + 1),
        [0, 0, 255],
        "no variant: the object's picture"
    );
    assert_ne!(pixel(&without, 38 * 4, 48 * 4), red, "and nothing past its box");
    // The pack's own variants, an `always` one and a `kaioken` one (listed first: the order does not decide)
    Arc::get_mut(&mut pack).unwrap().pose_art = vec![
        flat_variant(&dir, "own-kaioken", "battle-x-000", Kaioken, [255, 255, 0]),
        flat_variant(&dir, "own", "battle-x-000", Always, red),
    ];
    renderer.set_art(Some(pack));
    for kaioken in [false, true] {
        renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(kaioken, 0)));
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        let first = if kaioken { [255, 255, 0] } else { red };
        assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), first, "the sprite's pixels");
        assert_eq!(pixel(&drawn, 38 * 4, 48 * 4), first, "the margin's first pixel");
        assert_eq!(pixel(&drawn, 57 * 4 + 3, 67 * 4 + 3), first, "the margin's last pixel");
        assert_ne!(pixel(&drawn, 37 * 4 + 3, 50 * 4), first, "nothing past the margin");
        assert_eq!(
            pixel(&drawn, 5, 5),
            pixel(&without, 5, 5),
            "away from the pose nothing changes"
        );
    }
}

#[test]
fn a_data_packs_variant_of_the_same_art_wins_over_the_packs_own() {
    use ssd_mod::pose::Condition::{Always, Kaioken};
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let (red, yellow, green) = ([255, 0, 0], [255, 255, 0], [0, 255, 0]);
    let mut pack = variant_pack(&dir, &pose, None);
    let own = vec![flat_variant(&dir, "own", "battle-x-000", Always, red)];
    Arc::get_mut(&mut pack).unwrap().pose_art = own;
    let (off, on) = (
        Conditions::of_work_ram(&wram_with_kaioken(false, 0)),
        Conditions::of_work_ram(&wram_with_kaioken(true, 0)),
    );
    let corner = |pack: Arc<Pack>, conditions: Conditions| {
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        renderer.set_conditions(conditions);
        pixel(&renderer.render_hd(&frame, options(4)).unwrap(), 38 * 4, 48 * 4)
    };
    // A data pack's `kaioken` variant (the `ssj-kaioken` mod): drawn while the Kaioken is on, the pack's own art otherwise
    let mut kaioken = Arc::new((*pack).clone());
    Arc::get_mut(&mut kaioken).unwrap().variants =
        vec![flat_variant(&dir, "mod-kaioken", "battle-x-000", Kaioken, yellow)];
    assert_eq!(corner(Arc::new((*kaioken).clone()), on), yellow, "the Kaioken on: the data pack's");
    assert_eq!(corner(kaioken, off), red, "the Kaioken off: the pack's own art");
    // A data pack's `always` variant (the `ssj-goku-hd` mod): drawn in every frame
    let mut always = Arc::new((*pack).clone());
    Arc::get_mut(&mut always).unwrap().variants =
        vec![flat_variant(&dir, "mod-always", "battle-x-000", Always, green)];
    for conditions in [off, on] {
        assert_eq!(
            corner(Arc::new((*always).clone()), conditions),
            green,
            "the data pack's `always`"
        );
    }
    // ... even over an own `kaioken` variant while the Kaioken is on
    let own_kaioken = flat_variant(&dir, "own-kaioken", "battle-x-000", Kaioken, [0, 0, 128]);
    Arc::get_mut(&mut always).unwrap().pose_art.push(own_kaioken);
    assert_eq!(corner(always, on), green, "a data pack's variant beats any of the pack's own");
    // A data pack's variant whose picture is missing is not drawn: the pack's own art is
    let mut broken = Arc::new((*pack).clone());
    let mut missing = flat_variant(&dir, "mod-missing", "battle-x-000", Always, green);
    missing.file = dir.path().join("art/none.png").to_string_lossy().into_owned();
    Arc::get_mut(&mut broken).unwrap().variants = vec![missing];
    assert_eq!(corner(broken, off), red, "no picture: the pack's own art");
}

#[test]
fn the_packs_own_variant_of_super_saiyan_goku_is_drawn_for_the_pixels_he_shares_only_while_he_is_in_the_party()
 {
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/normal.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/other.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    let goku = |id: &str, key: u64, art: &str| Object {
        id: id.to_owned(),
        key,
        ..object(&pose, art, false)
    };
    let objects = vec![
        goku("battle-goku-000", pose.key, "art/normal.png"),
        goku("battle-goku-001", pose.key ^ 1, "art/other.png"),
        goku("battle-goku-ssj-000", pose.key, "art/ssj.png"),
    ];
    let mut pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    let always = ssd_mod::pose::Condition::Always;
    Arc::get_mut(&mut pack).unwrap().pose_art =
        vec![flat_variant(&dir, "hair", "battle-goku-ssj-000", always, [255, 255, 0])];
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    renderer.set_conditions(Conditions::default());
    let normal = renderer.render_hd(&frame, options(4)).unwrap();
    let blue = pixel(&normal, 41 * 4 + 1, 51 * 4 + 1);
    assert_eq!(blue, [0, 0, 255], "the normal Goku: the normal art");
    assert_ne!(pixel(&normal, 38 * 4, 48 * 4), [255, 255, 0], "no bigger art past the sprite");
    renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(false, 0)));
    let ssj = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&ssj, 41 * 4 + 1, 51 * 4 + 1), [255, 255, 0], "Super Saiyan Goku: bigger art");
    assert_eq!(pixel(&ssj, 38 * 4, 48 * 4), [255, 255, 0], "and its margin");
}

/// The pose of `fighter_log` with art made for its green (palette 1, colour 2): the pack's own art `art/o.png` is solid
/// green, and the override picture of its name (`battle-x-000`, 64 x 64 unless `size` says) a darker green in its lower
/// half. Returns the pack, the same pack with the folder of overrides, and a pack whose own art has the override's pixels.
fn override_packs(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    pose: &Pose,
    size: u32,
) -> (Arc<Pack>, Arc<Pack>, Arc<Pack>) {
    let darker = |_: u32, y: u32| if y < size / 2 { [0, 255, 0, 255] } else { [0, 190, 0, 255] };
    write_png(&dir.path().join("art/o.png"), 64, 64, |_, _| [0, 255, 0, 255]);
    write_png(&dir.path().join("art/r.png"), 64, 64, darker);
    write_png(&overrides.path().join("battle-x-000.png"), size, size, darker);
    let made_for_green = |art: &str| {
        let mut object = object(pose, art, false);
        object.palette = Palette { mask: 0b100, ..Palette::default() };
        object.palette.colors[2] = [0, 255, 0];
        object
    };
    let pack = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![made_for_green("art/o.png")]),
    );
    let overridden = Arc::new((*pack).clone().with_art_override(overrides.path()));
    let reference = pack_of(
        dir.path(),
        Vec::new(),
        ArtIndex::new(Vec::new(), vec![made_for_green("art/r.png")]),
    );
    (pack, overridden, reference)
}

fn drawn_with(pack: &Arc<Pack>, frame: &FrameLog) -> crate::HdFrame {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(pack)));
    renderer.render_hd(frame, options(4)).unwrap()
}

#[test]
fn an_override_picture_is_drawn_in_place_of_the_packs_art_recoloured_flashed_and_faded_as_the_packs_own()
 {
    let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (frame, pose) = fighter_log(40, 50);
    let (pack, overridden, reference) = override_packs(&dir, &overrides, &pose, 64);
    let lower = (41 * 4 + 1, 61 * 4 + 1);

    // The palette the art was made for: the picture as it is, where the pack's art is drawn
    let own = drawn_with(&pack, &frame);
    let drawn = drawn_with(&overridden, &frame);
    assert_eq!(pixel(&own, lower.0, lower.1), [0, 255, 0]);
    assert_eq!(pixel(&drawn, lower.0, lower.1), [0, 190, 0], "the override's darker half");
    assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [0, 255, 0]);
    assert_eq!(drawn, drawn_with(&reference, &frame), "as if the pack's own art were the picture");

    // Another palette (the pose in another fighter's colours), the white of a hit and a fade of the screen: every one draws
    // the override as the pack's own art of the same pixels is drawn, and not as the pack's art it replaces
    let mut red = frame.clone();
    red.cgram_start[0x80 + 16 + 2] = 0x001F;
    let mut white = frame.clone();
    white.cgram_start[0x80 + 16 + 2] = 0x7FFF;
    let mut faded = frame.clone();
    for entry in &mut faded.entries {
        entry.regs.brightness = 8;
    }
    for (name, frame) in [("red", &red), ("white", &white), ("faded", &faded)] {
        let drawn = drawn_with(&overridden, frame);
        assert_eq!(drawn, drawn_with(&reference, frame), "{name}");
        assert_ne!(
            pixel(&drawn, lower.0, lower.1),
            pixel(&drawn_with(&pack, frame), lower.0, lower.1),
            "{name}: the override is drawn"
        );
    }
    let [r, g, b] = pixel(&drawn_with(&overridden, &red), lower.0, lower.1);
    assert!(
        r > 150 && r < 255 && g == 0 && b == 0,
        "recoloured to red, still the darker: {r} {g} {b}"
    );
    let [r, g, b] = pixel(&drawn_with(&overridden, &white), lower.0, lower.1);
    assert!(
        r == g && g == b && r > 150 && r < 255,
        "the hit's white, still the darker: {r} {g} {b}"
    );
}

#[test]
fn an_override_picture_of_another_size_or_none_leaves_the_packs_art() {
    let (frame, pose) = fighter_log(40, 50);
    // A picture of 32 x 32 for art of 64 x 64: left out, the pack's art is drawn
    let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (pack, overridden, _) = override_packs(&dir, &overrides, &pose, 32);
    assert_eq!(overridden.art.objects(), pack.art.objects());
    assert_eq!(drawn_with(&overridden, &frame), drawn_with(&pack, &frame));
    // No picture of the pose's name
    let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (pack, _, _) = override_packs(&dir, &overrides, &pose, 64);
    std::fs::remove_file(overrides.path().join("battle-x-000.png")).unwrap();
    let none = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(drawn_with(&none, &frame), drawn_with(&pack, &frame));
    assert_eq!(pixel(&drawn_with(&none, &frame), 41 * 4 + 1, 61 * 4 + 1), [0, 255, 0]);
}

/// The fighter of [`fighter_log`] (`battle-x-000`, blue art `art/o.png`) in three packs: `own`, with the pack's own `always`
/// variant of it (green, margin 2 game pixels) and its Kaioken variant (yellow, margin 2); `overridden`, the same pack with
/// the folder `overrides` whose `variants\battle-x-000.png` is red, `size` x `size` art pixels (216 is the fighter's 64 grown
/// by 76 on each side); and `reference`, the pack whose own `always` variant is that red picture with the margin of 76 art
/// pixels (19 game pixels), the Kaioken's left as it is.
fn override_variant_packs(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    pose: &Pose,
    size: u32,
) -> (Arc<Pack>, Arc<Pack>, Arc<Pack>) {
    let (always, kaioken) = (ssd_mod::pose::Condition::Always, ssd_mod::pose::Condition::Kaioken);
    let mut own = variant_pack(dir, pose, None);
    Arc::get_mut(&mut own).unwrap().pose_art = vec![
        flat_variant(dir, "own", "battle-x-000", always, [0, 255, 0]),
        flat_variant(dir, "own-kaioken", "battle-x-000", kaioken, [255, 255, 0]),
    ];
    let sub = overrides.path().join(crate::pack::VARIANTS_DIR);
    std::fs::create_dir_all(&sub).unwrap();
    let picture = sub.join("battle-x-000.png");
    write_png(&picture, size, size, |_, _| [255, 0, 0, 255]);
    let overridden = Arc::new((*own).clone().with_art_override(overrides.path()));
    let mut reference = (*own).clone();
    reference.pose_art = vec![
        flat_variant(dir, "own-kaioken", "battle-x-000", kaioken, [255, 255, 0]),
        PoseVariant {
            art: "battle-x-000".to_owned(),
            when: always,
            margin: 19,
            file: std::path::absolute(&picture).unwrap().to_string_lossy().into_owned(),
        },
    ];
    (own, overridden, Arc::new(reference))
}

#[test]
fn a_picture_of_the_variants_folder_is_drawn_over_its_box_and_a_margin_of_76_art_pixels_in_place_of_the_packs_own()
 {
    let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // The 16 x 16 fighter at (40, 50): the picture covers 21 to 74, 31 to 84 (19 game pixels on each side)
    let (frame, pose) = fighter_log(40, 50);
    let (own, overridden, _) = override_variant_packs(&dir, &overrides, &pose, 216);
    let red = [255, 0, 0];
    let mut renderer = Renderer::new();
    let plain = renderer.render_hd(&frame, options(4)).unwrap();
    renderer.set_art(Some(Arc::clone(&own)));
    let packs = renderer.render_hd(&frame, options(4)).unwrap();
    assert_eq!(pixel(&packs, 41 * 4 + 1, 51 * 4 + 1), [0, 255, 0], "the pack's own variant");
    renderer.set_art(Some(overridden));
    for on in [false, true] {
        renderer.set_conditions(Conditions::of_work_ram(&wram_with_kaioken(on, 0)));
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        if on {
            // A variant for a condition is drawn while it holds, over the `always` one, as over the pack's own
            assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), [255, 255, 0], "the Kaioken's");
            continue;
        }
        assert_eq!(pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), red, "the sprite's pixels");
        assert_eq!(pixel(&drawn, 21 * 4, 31 * 4), red, "the margin's first pixel");
        assert_eq!(pixel(&drawn, 74 * 4 + 3, 84 * 4 + 3), red, "the margin's last pixel");
        assert_ne!(pixel(&drawn, 20 * 4 + 3, 50 * 4), red, "nothing past the margin on the left");
        assert_ne!(pixel(&drawn, 40 * 4, 30 * 4 + 3), red, "nor above it");
        assert_eq!(pixel(&drawn, 5, 5), pixel(&plain, 5, 5), "away from the pose nothing changes");
    }
}

#[test]
fn a_picture_of_the_variants_folder_is_drawn_as_the_packs_own_variant_faded_and_over_blank_text_cells()
 {
    let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (frame, pose) = fighter_log(40, 50);
    let (_, overridden, reference) = override_variant_packs(&dir, &overrides, &pose, 216);
    // Another palette, the white of a hit and a fade of the screen: every frame as the pack's own variant of the picture
    let mut red = frame.clone();
    red.cgram_start[0x80 + 16 + 2] = 0x001F;
    let mut white = frame.clone();
    white.cgram_start[0x80 + 16 + 2] = 0x7FFF;
    let mut faded = frame.clone();
    for entry in &mut faded.entries {
        entry.regs.brightness = 8;
    }
    for (name, frame) in [("plain", &frame), ("red", &red), ("white", &white), ("faded", &faded)] {
        assert_eq!(drawn_with(&overridden, frame), drawn_with(&reference, frame), "{name}");
    }
    let [r, g, b] = pixel(&drawn_with(&overridden, &faded), 21 * 4, 31 * 4);
    assert!(r > 0 && r < 255 && g == 0 && b == 0, "the fade shades the margin: {r} {g} {b}");
    // In front of the white BG3 spaces of an aura: the HD text leaves the margin (the Super Saiyan's raised hair)
    let spaces = fighter_on_spaces(false);
    let drawn_on_spaces = |pack: &Arc<Pack>| {
        let mut renderer = Renderer::new();
        renderer.set_font(Some(space_font()));
        renderer.set_art(Some(Arc::clone(pack)));
        renderer.render_hd(&spaces, options(4)).unwrap()
    };
    let drawn = drawn_on_spaces(&overridden);
    assert_eq!(pixel(&drawn, 21 * 4, 31 * 4), [255, 0, 0], "the margin's first pixel, on a space");
    assert_eq!(pixel(&drawn, 45 * 4 + 2, 31 * 4 + 1), [255, 0, 0], "the margin above the sprite");
    assert_eq!(
        pixel(&drawn, 20 * 4 + 3, 50 * 4),
        [0xFF; 3],
        "past the margin the cells keep their white"
    );
    assert_eq!(drawn, drawn_on_spaces(&reference));
}

#[test]
fn a_picture_of_the_variants_folder_of_another_size_or_none_leaves_the_packs_own_variant() {
    let (frame, pose) = fighter_log(40, 50);
    for on in [false, true] {
        let conditions = Conditions::of_work_ram(&wram_with_kaioken(on, 0));
        let drawn = |pack: &Arc<Pack>| {
            let mut renderer = Renderer::new();
            renderer.set_art(Some(Arc::clone(pack)));
            renderer.set_conditions(conditions);
            renderer.render_hd(&frame, options(4)).unwrap()
        };
        // 80 x 80 (the pack's own margin of 2 game pixels) and 220 x 216: left out, the pack's own variant is drawn
        for size in [80, 220] {
            let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            let (own, overridden, _) = override_variant_packs(&dir, &overrides, &pose, size);
            assert_eq!(overridden.pose_art, own.pose_art, "{size}");
            assert_eq!(drawn(&overridden), drawn(&own), "{size}");
        }
        // No folder `variants`, an empty one, and no folder at all: every frame the same
        let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (own, _, _) = override_variant_packs(&dir, &overrides, &pose, 216);
        let sub = overrides.path().join(crate::pack::VARIANTS_DIR);
        std::fs::remove_file(sub.join("battle-x-000.png")).unwrap();
        let empty = Arc::new((*own).clone().with_art_override(overrides.path()));
        std::fs::remove_dir(&sub).unwrap();
        let none = Arc::new((*own).clone().with_art_override(overrides.path()));
        let absent = Arc::new((*own).clone().with_art_override(&overrides.path().join("absent")));
        for pack in [&empty, &none, &absent] {
            assert_eq!(pack.pose_art, own.pose_art);
            assert_eq!(drawn(pack), drawn(&own), "Kaioken {on}");
        }
    }
}

/// The pose of [`fighter_log`] as a pose of Super Vegeta the pack has only Vegeta's art for (patch 0049): `name`, blue,
/// in `vegeta` (his palette as the art has it); Super Vegeta's palette is known from the art of another pose
/// (`battle-super-vegeta-000`, and his arm `battle-super-vegeta-001`, no hair), and `art-override\super-vegeta\` holds a
/// picture of the pose's name, magenta. Returns the pack without the folder and the pack with it.
fn super_vegeta_override_packs(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    pose: &Pose,
    name: &str,
    vegeta: Palette,
) -> (Arc<Pack>, Arc<Pack>) {
    write_png(&dir.path().join("art/vegeta.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    write_png(&overrides.path().join(format!("super-vegeta/{name}.png")), 64, 64, |_, _| {
        [255, 0, 255, 255]
    });
    let vegeta =
        Object { id: name.to_owned(), palette: vegeta, ..object(pose, "art/vegeta.png", false) };
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: pose.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        width: 8,
        height: 8,
        ..object(pose, "art/vegeta.png", false)
    };
    // and a piece of him with no hair (his arm: the outline and colour 2), the same colours as Vegeta's there
    let mut arm = Object {
        id: "battle-super-vegeta-001".to_owned(),
        key: pose.key ^ 2,
        ..super_vegeta.clone()
    };
    arm.palette.mask = 1 << 1 | 1 << 2;
    let pack =
        pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![vegeta, super_vegeta, arm]));
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    (pack, with)
}

/// The fighter of [`fighter_log`] drawn from `pack`: the colour at its middle, the art drawn (an index of the pack), whether
/// it was recoloured, and the whole picture.
fn drawn_fighter(pack: &Arc<Pack>, frame: &FrameLog) -> ([u8; 3], Option<u32>, bool, Vec<u8>) {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(pack)));
    let drawn = renderer.render_hd(frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let fighter = stats.poses.iter().find(|p| p.fighter).unwrap();
    (pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1), fighter.art, fighter.recoloured, drawn.rgba.clone())
}

/// Super Vegeta's palette in the frame of [`fighter_log`]: yellow and orange hair at 14 and 15.
fn super_vegeta_colours(frame: &mut FrameLog) {
    frame.cgram_start[0x80 + 16 + 14] = 0x03FF; // yellow
    frame.cgram_start[0x80 + 16 + 15] = 0x11DF; // orange
}

#[test]
fn super_vegeta_s_own_picture_of_a_pose_whose_art_shows_no_hair_is_drawn_in_his_colours_and_never_in_vegeta_s()
 {
    // `battle-vegeta-803` (both arms up, the head thrown back): the game draws Super Vegeta so with Vegeta's tiles in his
    // own palette, and the pack's art is Vegeta's, with the shoulder pads. It uses no colour of the hair, so it was drawn
    // in his colours as it is, pads and all. Ming's picture of the pose without the pads
    // (`art-override\super-vegeta\battle-vegeta-803.png`) is drawn there; Vegeta in his own colours keeps the pack's art,
    // the pads (the picture's name is his). Vegeta's colours and Super Vegeta's are the same on the colours the pose uses
    // (and on those of his arm's art): only the hair of the frame's palette tells them apart
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let mut no_hair = vegeta_palette(None);
    no_hair.mask = 1 << 1 | 1 << 2;
    let (pack, with) =
        super_vegeta_override_packs(&dir, &overrides, &pose, "battle-vegeta-803", no_hair);
    let added = &with.art.objects()[3];
    assert_eq!(added.id, "battle-vegeta-803@super-vegeta");
    assert_eq!(
        with.art.objects()[..3],
        pack.art.objects()[..],
        "Vegeta's own pose is left as it was"
    );
    // Vegeta's colours (black hair): his art, the same picture to the pixel as without the folder
    let (colour, art, _, vegeta) = drawn_fighter(&with, &frame);
    assert_eq!(
        (colour, art),
        ([0, 0, 255], Some(0)),
        "Vegeta: the pack's art, not Super Vegeta's picture"
    );
    assert_eq!(vegeta, drawn_fighter(&pack, &frame).3, "Vegeta: as without the folder");
    // Super Vegeta's: the picture, in its own colours (it was made in them); without the folder Vegeta's art
    super_vegeta_colours(&mut frame);
    let (colour, art, recoloured, _) = drawn_fighter(&with, &frame);
    assert_eq!(
        (colour, art, recoloured),
        ([255, 0, 255], Some(3), false),
        "Super Vegeta: the picture"
    );
    assert_eq!(
        drawn_fighter(&pack, &frame).1,
        Some(0),
        "without the folder: Vegeta's art, with the pads"
    );
    // Two other colours of hair (neither his nor Vegeta's): Vegeta's art, as without the folder
    frame.cgram_start[0x80 + 16 + 15] = 0x7C00; // blue
    let (_, art, _, other) = drawn_fighter(&with, &frame);
    assert_eq!(art, Some(0), "not his palette: Vegeta's art");
    assert_eq!(other, drawn_fighter(&pack, &frame).3, "other colours: as without the folder");
}

#[test]
fn super_vegeta_s_own_picture_of_a_pose_whose_art_shows_the_hair_is_drawn_in_his_colours_only() {
    // `gallery-party-vegeta-u80-056` (his upper body at the screen's top edge): Vegeta's art with his hair, a dark grey
    // apart from the outline, so it recolours to Super Vegeta's yellow hair, pads and all. Ming's picture of it is drawn
    // in Super Vegeta's colours, Vegeta's art in Vegeta's and in any other
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let grey = Some(([40, 40, 40], [60, 60, 60]));
    let name = "gallery-party-vegeta-u80-056";
    let (pack, with) =
        super_vegeta_override_packs(&dir, &overrides, &pose, name, vegeta_palette(grey));
    let (_, art, _, vegeta) = drawn_fighter(&with, &frame);
    assert_eq!(art, Some(0), "Vegeta: his art");
    assert_eq!(vegeta, drawn_fighter(&pack, &frame).3, "as without the folder");
    super_vegeta_colours(&mut frame);
    let (colour, art, recoloured, _) = drawn_fighter(&with, &frame);
    assert_eq!(
        (colour, art, recoloured),
        ([255, 0, 255], Some(3), false),
        "Super Vegeta: the picture"
    );
    assert_eq!(
        drawn_fighter(&pack, &frame).1,
        Some(0),
        "without the folder: Vegeta's art recoloured"
    );
}

#[test]
fn super_vegeta_s_keyed_picture_of_a_pose_of_no_art_is_drawn_in_his_colours_and_vegeta_keeps_the_game_s_sprite()
 {
    // Super Vegeta's back to Goku's Kamehameha (key `…9b50`, and the throw after it) is drawn with Vegeta's tiles the pack
    // has no art for, so the game's sprite showed the pads. `art-override\super-vegeta\key-<key>.png` with the pose's
    // pixels in `key-<key>.txt` is drawn in his colours; in Vegeta's (black hair), and in any other, the pose keeps the
    // game's sprite, as without the folder
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/vegeta.png"), 32, 32, |_, _| [0, 0, 255, 255]);
    let name = format!("super-vegeta/key-{:016x}", pose.key);
    write_png(&overrides.path().join(format!("{name}.png")), 64, 64, |_, _| [255, 0, 255, 255]);
    let row = "2".repeat(16) + "\n";
    std::fs::write(overrides.path().join(format!("{name}.txt")), row.repeat(16)).unwrap();
    // Super Vegeta's palette is known from his art of another pose
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: pose.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        width: 8,
        height: 8,
        ..object(&pose, "art/vegeta.png", false)
    };
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![super_vegeta]));
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(with.art.objects().len(), 2, "the pose added");
    assert_eq!(with.art.objects()[1].id, format!("key-{:016x}@super-vegeta", pose.key));
    // Vegeta's colours (black hair): the game's sprite, the same picture to the pixel as without the folder
    let (colour, art, _, vegeta) = drawn_fighter(&with, &frame);
    assert_eq!((colour, art), ([0, 255, 0], None), "Vegeta: the game's sprite");
    assert_eq!(vegeta, drawn_fighter(&pack, &frame).3, "Vegeta: as without the folder");
    // Super Vegeta's: the picture, in its own colours; without the folder the game's sprite
    super_vegeta_colours(&mut frame);
    let (colour, art, recoloured, _) = drawn_fighter(&with, &frame);
    assert_eq!(
        (colour, art, recoloured),
        ([255, 0, 255], Some(1), false),
        "Super Vegeta: the picture"
    );
    assert_eq!(drawn_fighter(&pack, &frame).1, None, "without the folder: the game's sprite");
    // Two other colours of hair: the game's sprite, as without the folder
    frame.cgram_start[0x80 + 16 + 15] = 0x7C00; // blue
    let (_, art, _, other) = drawn_fighter(&with, &frame);
    assert_eq!(art, None, "not his palette: the game's sprite");
    assert_eq!(other, drawn_fighter(&pack, &frame).3, "other colours: as without the folder");
}

#[test]
fn a_keyed_picture_with_its_pose_and_no_pixels_draws_every_frame_as_with_its_pixels() {
    // A release ships `key-<key>.pose` in place of the original sprite's pixels `key-<key>.txt`. The frames are the
    // same to the pixel: in Super Vegeta's colours (the picture), in Vegeta's and in others (the game's sprite), and with
    // an effect behind him where his sprite is transparent (drawn from back to front)
    let dir = tempfile::tempdir().unwrap();
    let (with_pixels, with_pose) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut frame, pose) = fighter_log(40, 50);
    write_png(&dir.path().join("art/vegeta.png"), 32, 32, |_, _| [0, 0, 255, 255]);
    let name = format!("super-vegeta/key-{:016x}", pose.key);
    let pixels = ("2".repeat(16) + "\n").repeat(16);
    for overrides in [&with_pixels, &with_pose] {
        write_png(&overrides.path().join(format!("{name}.png")), 64, 64, |x, y| {
            [255, (x * 4) as u8, (y * 4) as u8, 255]
        });
    }
    std::fs::write(with_pixels.path().join(format!("{name}.txt")), &pixels).unwrap();
    let pose_text = crate::pack::keyed_pose_text(&pixels).unwrap();
    std::fs::write(with_pose.path().join(format!("{name}.pose")), pose_text).unwrap();
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: pose.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        width: 8,
        height: 8,
        ..object(&pose, "art/vegeta.png", false)
    };
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![super_vegeta]));
    let a = Arc::new((*pack).clone().with_art_override(with_pixels.path()));
    let b = Arc::new((*pack).clone().with_art_override(with_pose.path()));
    assert_eq!(b.art.objects().len(), 2, "the pose added without its pixels");
    let vegeta = drawn_fighter(&b, &frame);
    assert_eq!(vegeta.1, None, "Vegeta: the game's sprite");
    assert_eq!(drawn_fighter(&a, &frame), vegeta, "Vegeta's colours");
    super_vegeta_colours(&mut frame);
    let his = drawn_fighter(&b, &frame);
    assert_eq!(his.1, Some(1), "Super Vegeta: the picture");
    assert_eq!(drawn_fighter(&a, &frame), his, "Super Vegeta's colours");
    frame.cgram_start[0x80 + 16 + 15] = 0x7C00; // blue
    assert_eq!(drawn_fighter(&a, &frame), drawn_fighter(&b, &frame), "other colours");
    // an effect behind him where his sprite is transparent
    let (frame, fighter, effect) = effect_behind_a_gap_log(true);
    let drawn = |pose_only: bool| {
        let (dir, overrides) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let pack = super_vegeta_and_waves_pack_as(
            &dir,
            &overrides,
            &fighter,
            &[&effect],
            |x, y| if x < 8 && y < 32 { [0, 0, 0, 0] } else { [255, 0, 255, 255] },
            pose_only,
        );
        let mut renderer = Renderer::new();
        renderer.set_art(Some(pack));
        let drawn = renderer.render_hd(&frame, options(4)).unwrap();
        let art = renderer.art_stats().poses.iter().find(|p| p.fighter).unwrap().art;
        (art, drawn.rgba.clone())
    };
    let (pixels_art, pixels_frame) = drawn(false);
    assert_eq!(pixels_art, Some(2), "his picture");
    assert!(drawn(true) == (pixels_art, pixels_frame), "the effect behind him: the same frame");
}

/// The frame of [`fighter_log`] in Super Vegeta's colours with an effect behind the fighter: sprite 1, 16 x 16, palette 4,
/// solid colour 3 (red), four pixels left of him (`x` 36 to 51, the fighter 40 to 55), and the effect's pose. The fighter
/// is in front (sprite 0), so the original shows him where both are.
fn effect_behind_log() -> (FrameLog, Pose, Pose) {
    let (mut frame, _) = fighter_log(40, 50);
    super_vegeta_colours(&mut frame);
    let (frame, fighter, effect) = with_effect(frame, false, &[2, 3, 18, 19]);
    assert_eq!((effect.org.0, effect.w, effect.h), (36, 16, 16));
    (frame, fighter, effect)
}

/// `frame` (of [`fighter_log`] at `(40, 50)`) with an effect four pixels left of the fighter: a 16 x 16 sprite at `x` 36
/// to 51, palette 4, solid colour 3 (red) in its characters `tiles` (2 and 3 its top half, 18 and 19 its bottom half), the
/// others transparent. The effect is OAM entry 0 and the fighter entry 1 (the effect in front of him) with `in_front`,
/// else the fighter 0 and the effect 1. Returns the frame and the poses of the fighter and of the effect found in it.
fn with_effect(mut frame: FrameLog, in_front: bool, tiles: &[usize]) -> (FrameLog, Pose, Pose) {
    let effect = [36, 50, 2, 4 << 1];
    if in_front {
        let fighter = [frame.oam_low[0], frame.oam_low[1], frame.oam_low[2], frame.oam_low[3]];
        frame.oam_low[0..4].copy_from_slice(&effect);
        frame.oam_low[4..8].copy_from_slice(&fighter);
    } else {
        frame.oam_low[4..8].copy_from_slice(&effect);
    }
    frame.oam_high[0] |= 0b10 << 2;
    for &tile in tiles {
        for row in 0..8 {
            frame.vram[tile * 16 + row] = 0xFFFF;
        }
    }
    frame.cgram_start[0x80 + 4 * 16 + 3] = 0x001F; // red
    let sprites = poses::sprites(&frame);
    let found: Vec<Pose> =
        poses::poses(&frame, &sprites).into_iter().filter(|pose| !pose.field).collect();
    let fighter = found.iter().find(|pose| pose.fighter).expect("the fighter").clone();
    let effect = found.iter().find(|pose| !pose.fighter).expect("the effect").clone();
    assert_eq!((fighter.first, effect.first), if in_front { (1, 0) } else { (0, 1) });
    (frame, fighter, effect)
}

/// The pack of red art for `fighter` (in Super Vegeta's colours) and for `effect` (an energy wave), with the folder
/// `overrides` whose `super-vegeta\key-<key>.png` is `picture` (with its pixels, the fighter's): his own picture of the
/// pose, drawn in place of the game's sprite (patch 0049).
fn super_vegeta_and_wave_pack(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    fighter: &Pose,
    effect: &Pose,
    picture: impl Fn(u32, u32) -> [u8; 4],
) -> Arc<Pack> {
    super_vegeta_and_waves_pack(dir, overrides, fighter, &[effect], picture)
}

/// [`super_vegeta_and_wave_pack`] with red art for each of `effects`.
fn super_vegeta_and_waves_pack(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    fighter: &Pose,
    effects: &[&Pose],
    picture: impl Fn(u32, u32) -> [u8; 4],
) -> Arc<Pack> {
    super_vegeta_and_waves_pack_as(dir, overrides, fighter, effects, picture, false)
}

/// [`super_vegeta_and_waves_pack`], the picture's pose given by `key-<key>.pose` in place of its pixels with `pose_only`
/// (what a release ships, `crate::pack::keyed_pose_text`).
fn super_vegeta_and_waves_pack_as(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    fighter: &Pose,
    effects: &[&Pose],
    picture: impl Fn(u32, u32) -> [u8; 4],
    pose_only: bool,
) -> Arc<Pack> {
    write_png(&dir.path().join("art/red.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let name = format!("super-vegeta/key-{:016x}", fighter.key);
    write_png(&overrides.path().join(format!("{name}.png")), 64, 64, picture);
    let pixels: String = fighter
        .arr
        .chunks(fighter.w)
        .map(|row| {
            row.iter().map(|&c| char::from_digit(u32::from(c), 16).unwrap()).collect::<String>()
                + "\n"
        })
        .collect();
    if pose_only {
        let pose = crate::pack::keyed_pose_text(&pixels).unwrap();
        std::fs::write(overrides.path().join(format!("{name}.pose")), pose).unwrap();
    } else {
        std::fs::write(overrides.path().join(format!("{name}.txt")), pixels).unwrap();
    }
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: fighter.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        ..object(fighter, "art/red.png", false)
    };
    let mut objects = vec![super_vegeta];
    for (n, effect) in effects.iter().enumerate() {
        let id = format!("battle-fx-{n:03}");
        objects.push(Object { id, ..object(effect, "art/red.png", false) });
    }
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    Arc::new((*pack).clone().with_art_override(overrides.path()))
}

/// The frame of [`fighter_log`] with the fighter's top left two columns transparent in rows 0 to 7 (`x` 40 and 41) and
/// the effect of [`with_effect`] behind him: the original shows the effect there, the fighter's art grows over it.
fn effect_behind_a_gap_log(super_vegeta: bool) -> (FrameLog, Pose, Pose) {
    let (mut frame, _) = fighter_log(40, 50);
    if super_vegeta {
        super_vegeta_colours(&mut frame);
    }
    for row in 0..8 {
        frame.vram[row] = 0x3F00;
    }
    let (frame, fighter, effect) = with_effect(frame, false, &[2, 3, 18, 19]);
    assert_eq!((fighter.org.0, fighter.w, fighter.h), (40, 16, 16));
    assert_eq!(fighter.arr[3 * 16..3 * 16 + 3], [0, 0, 2], "the gap in his silhouette");
    (frame, fighter, effect)
}

/// The fighter's art of [`super_vegeta_s_art_leaves_what_is_behind_it_shown_where_it_is_transparent`]: magenta, its top
/// left transparent where the original's fighter covers the effect (columns 0 to 5, rows 0 to 7 of the pose), half
/// transparent in columns 6 and 7 there, opaque elsewhere.
fn part_transparent(x: u32, y: u32) -> [u8; 4] {
    match (x / 4, y / 4) {
        (0..=5, 0..=7) => [0, 0, 0, 0],
        (6..=7, 0..=7) => [255, 0, 255, 128],
        _ => [255, 0, 255, 255],
    }
}

#[test]
fn super_vegeta_s_art_leaves_what_is_behind_it_shown_where_it_is_transparent() {
    // Super Vegeta's picture without the shoulder pads (`art-override\super-vegeta\`) does not cover the pixels of the
    // game's sprite where the pads were: an energy wave behind him was hidden there by the sprite, and with the sprite
    // hidden too nothing drew it, so the background showed through the wave in the pads' shape. Where his art is
    // transparent the effect's art is drawn; where it is half transparent the two are mixed; where it is opaque it
    // covers the effect as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, fighter, effect) = effect_behind_log();
    write_png(&dir.path().join("art/red.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let name = format!("super-vegeta/key-{:016x}", fighter.key);
    write_png(&overrides.path().join(format!("{name}.png")), 64, 64, part_transparent);
    let row = "2".repeat(16) + "\n";
    std::fs::write(overrides.path().join(format!("{name}.txt")), row.repeat(16)).unwrap();
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let super_vegeta = Object {
        id: "battle-super-vegeta-000".to_owned(),
        key: fighter.key ^ 1,
        palette: vegeta_palette(Some((yellow, orange))),
        ..object(&fighter, "art/red.png", false)
    };
    let wave = Object { id: "battle-fx-000".to_owned(), ..object(&effect, "art/red.png", false) };
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![super_vegeta, wave]));
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(&with)));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let arts: Vec<Option<u32>> = stats.poses.iter().filter(|p| !p.field).map(|p| p.art).collect();
    assert_eq!(arts, [Some(2), Some(1)], "his picture and the wave's art");
    // (game pixel (x, row) of the fighter's pose, sampled in the middle of its 4 x 4 art pixels)
    let at = |x: usize, row: usize| pixel(&drawn, (40 + x) * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(2, 3), [255, 0, 0], "his art transparent: the wave behind him");
    assert_eq!(at(2, 12), [255, 0, 255], "his art opaque: he covers the wave");
    assert_eq!(at(10, 3), [255, 0, 255], "his art opaque where no wave is behind");
    assert_eq!(at(6, 3), [255, 0, 128], "half transparent: the two mixed, no dark or light edge");
    assert_eq!(pixel(&drawn, 37 * 4 + 2, 52 * 4 + 2), [255, 0, 0], "the wave beside him");
}

#[test]
fn art_of_the_pack_still_hides_what_is_behind_its_sprites_where_it_is_transparent() {
    // Only a picture of a folder of `art-override\` that adds poses (`super-vegeta\`, `goku\`, `ending-goku\`:
    // `Object::override_only`) lets what is behind its sprites show: the pack's own art of the same pose, transparent in
    // the same place, draws as it did, the frame unchanged
    let dir = tempfile::tempdir().unwrap();
    let (frame, fighter, effect) = effect_behind_log();
    write_png(&dir.path().join("art/red.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/fighter.png"), 64, 64, part_transparent);
    let own = object(&fighter, "art/fighter.png", false);
    let wave = Object { id: "battle-fx-000".to_owned(), ..object(&effect, "art/red.png", false) };
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![own, wave]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let at = |x: usize, row: usize| pixel(&drawn, (40 + x) * 4 + 2, (51 + row) * 4 + 2);
    assert_ne!(at(2, 3), [255, 0, 0], "the wave is not drawn behind the pack's art");
    assert_eq!(at(2, 12), [255, 0, 255]);
}

#[test]
fn super_vegeta_s_art_past_the_game_s_silhouette_is_not_drawn_over_by_an_effect_behind_him() {
    // Where his sprite is transparent next to its pixels and an effect behind him shows, his picture grows over the
    // effect, as any pose's art over what is behind it: the poses are drawn from back to front, so the effect's art,
    // drawn before his, does not cut his edge back (in the order the poses were found, the fighter first, it did)
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, fighter, effect) = effect_behind_a_gap_log(true);
    let with =
        super_vegeta_and_wave_pack(&dir, &overrides, &fighter, &effect, |_, _| [255, 0, 255, 255]);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(with));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    assert_eq!(stats.poses.iter().find(|p| p.fighter).unwrap().art, Some(2), "his picture");
    let at = |x: usize, row: usize| pixel(&drawn, (40 + x) * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(0, 3), [255, 0, 255], "his art grown over the wave behind him");
    assert_eq!(at(1, 3), [255, 0, 255]);
    assert_eq!(pixel(&drawn, 37 * 4 + 2, 54 * 4 + 2), [255, 0, 0], "the wave past his reach");
}

#[test]
fn art_of_the_pack_is_drawn_in_the_order_the_poses_were_found() {
    // With no picture of a folder that adds poses in the frame the poses are drawn in the order they were found, as before
    // patch 0049's back-to-front order: the fighter's art grown over the effect behind him is drawn over by the effect's,
    // drawn after it
    let dir = tempfile::tempdir().unwrap();
    let (frame, fighter, effect) = effect_behind_a_gap_log(false);
    write_png(&dir.path().join("art/red.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/fighter.png"), 64, 64, |_, _| [255, 0, 255, 255]);
    let own = object(&fighter, "art/fighter.png", false);
    let wave = Object { id: "battle-fx-000".to_owned(), ..object(&effect, "art/red.png", false) };
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![own, wave]));
    let mut renderer = Renderer::new();
    renderer.set_art(Some(pack));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let at = |x: usize, row: usize| pixel(&drawn, (40 + x) * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(0, 3), [255, 0, 0], "the effect's art, drawn after the fighter's");
    assert_eq!(at(2, 3), [255, 0, 255], "his own pixels");
}

#[test]
fn an_effect_in_front_of_super_vegeta_s_picture_grows_over_it() {
    // A spark of a hit on him, his own energy wave: an effect in front of the fighter (OAM entry 0) whose bottom right is
    // transparent, where the original shows him. The effect's art grows past its pixels over him, as over any pose
    // behind it: his picture, drawn before it (from back to front), does not draw over that edge
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, _) = fighter_log(40, 50);
    super_vegeta_colours(&mut frame);
    let (frame, fighter, effect) = with_effect(frame, true, &[2, 3, 18]);
    assert_eq!((effect.org.0, effect.w, effect.h), (36, 16, 16));
    let with =
        super_vegeta_and_wave_pack(&dir, &overrides, &fighter, &effect, |_, _| [255, 0, 255, 255]);
    let mut renderer = Renderer::new();
    renderer.set_art(Some(with));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    assert_eq!(stats.poses.iter().find(|p| p.fighter).unwrap().art, Some(2), "his picture");
    // (game pixel (x, row): `x` of the screen, `row` of the poses' box)
    let at = |x: usize, row: usize| pixel(&drawn, x * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(44, 12), [255, 0, 0], "the effect's art grown over him");
    assert_eq!(at(45, 12), [255, 0, 0]);
    assert_eq!(at(49, 12), [255, 0, 255], "his picture past the effect's reach");
    assert_eq!(at(42, 3), [255, 0, 0], "the effect in front of him");
}

#[test]
fn super_vegeta_between_two_effects_is_drawn_from_back_to_front_not_in_the_order_the_poses_were_found()
 {
    // An effect behind him (OAM entry 2) under the gap at his top left, and one in front of him (entry 0) over his bottom
    // rows, its top right transparent. The poses are found the fighter first, then the effects in their OAM order (front,
    // back): neither that order, nor its reverse, nor his picture drawn last is back to front. His art grows over the
    // effect behind him (drawn after it), the front effect's grows over him (drawn after him)
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, _) = fighter_log(40, 50);
    super_vegeta_colours(&mut frame);
    for row in 0..8 {
        frame.vram[row] = 0x3F00;
    }
    let fighter = [frame.oam_low[0], frame.oam_low[1], frame.oam_low[2], frame.oam_low[3]];
    // (16 x 16, palette 4: the front one's characters 4 and 21 (5 and 20 transparent), x 40 to 55 and screen rows 63 to 78, four
    // rows past the bottom of the one behind, characters 2 to 3 and 18 to 19, x 36 to 51 and screen rows 43 to 58: two
    // poses, not one)
    frame.oam_low[0..4].copy_from_slice(&[40, 62, 4, 4 << 1]);
    frame.oam_low[4..8].copy_from_slice(&fighter);
    frame.oam_low[8..12].copy_from_slice(&[36, 42, 2, 4 << 1]);
    frame.oam_high[0] = 0b10_10_10;
    for tile in [2_usize, 3, 18, 19, 4, 21] {
        for row in 0..8 {
            frame.vram[tile * 16 + row] = 0xFFFF;
        }
    }
    frame.cgram_start[0x80 + 4 * 16 + 3] = 0x001F; // red
    let sprites = poses::sprites(&frame);
    let found: Vec<Pose> =
        poses::poses(&frame, &sprites).into_iter().filter(|pose| !pose.field).collect();
    let firsts: Vec<(bool, u8)> = found.iter().map(|pose| (pose.fighter, pose.first)).collect();
    assert_eq!(
        firsts,
        [(true, 1), (false, 0), (false, 2)],
        "found: him, the front effect, the back one"
    );
    let (fighter, front, behind) = (&found[0], &found[1], &found[2]);
    assert_eq!(fighter.arr[3 * 16..3 * 16 + 3], [0, 0, 2], "the gap in his silhouette");
    let with = super_vegeta_and_waves_pack(&dir, &overrides, fighter, &[front, behind], |_, _| {
        [255, 0, 255, 255]
    });
    let mut renderer = Renderer::new();
    renderer.set_art(Some(with));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    let art: Vec<Option<u32>> = stats.poses.iter().filter(|p| !p.field).map(|p| p.art).collect();
    assert_eq!(art, [Some(3), Some(1), Some(2)], "his picture and the two effects' art");
    let at = |x: usize, row: usize| pixel(&drawn, x * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(40, 3), [255, 0, 255], "his art grown over the effect behind him");
    assert_eq!(at(48, 12), [255, 0, 0], "the front effect's art grown over him");
    assert_eq!(at(53, 12), [255, 0, 255], "his picture past the front effect's reach");
    assert_eq!(at(37, 3), [255, 0, 0], "the effect behind him past his reach");
}

#[test]
fn a_picture_of_goku_s_folder_leaves_what_is_behind_it_shown_where_it_is_transparent() {
    // Any folder of `art-override\` that adds poses (`Object::override_only`), not Super Vegeta's alone: the normal Goku's
    // picture of `art-override\goku\` (patch 0051), transparent where the game's sprite covers an energy wave behind him,
    // shows the wave there and covers it where it is opaque. What this pins down is that a picture of `goku\` is one whose
    // sprites are open (drawn behind, `cells_behind`), not the order the poses are drawn in: the pixels it checks are
    // the same in any order (see `super_vegeta_between_two_effects_is_drawn_from_back_to_front_not_in_the_order_the_poses_were_found`)
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, _) = fighter_log(40, 50);
    let (frame, fighter, effect) = with_effect(frame, false, &[2, 3, 18, 19]);
    write_png(&dir.path().join("art/red.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    let mut objects = powering_up_pack(&dir, &fighter).art.objects().to_vec();
    let wave = Object { id: "battle-fx-000".to_owned(), ..object(&effect, "art/red.png", false) };
    objects.push(wave);
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects));
    write_png(&overrides.path().join("goku/battle-goku-ssj-020.png"), 64, 64, part_transparent);
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(with.art.objects()[3].id, "battle-goku-ssj-020@goku");
    let mut renderer = Renderer::new();
    renderer.set_art(Some(with));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let stats = renderer.art_stats();
    assert_eq!(stats.poses.iter().find(|p| p.fighter).unwrap().art, Some(3), "his picture");
    let at = |x: usize, row: usize| pixel(&drawn, (40 + x) * 4 + 2, (51 + row) * 4 + 2);
    assert_eq!(at(2, 3), [255, 0, 0], "his art transparent: the wave behind him");
    assert_eq!(at(2, 12), [255, 0, 255], "his art opaque: he covers the wave");
    assert_eq!(at(6, 3), [255, 0, 128], "half transparent: the two mixed");
}

#[test]
fn super_vegeta_flying_through_space_is_drawn_from_his_picture_of_battle_vegeta_001_in_his_colours_only()
 {
    // After the party loses the hidden fight, the game draws Super Vegeta flying through space with the tiles of
    // `battle-vegeta-001` in his colours, OAM palette 0: a field group with the key of that battle pose. Ming's picture
    // of the pose (`art-override\super-vegeta\battle-vegeta-001.png`) is drawn there; in Vegeta's colours, another
    // pose's field group in Super Vegeta's, or without the folder, nothing is found for the group, as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (_, fighter) = fighter_log(40, 50);
    let fighter = Pose { key: super::ENDING_SPACE_VEGETA, ..fighter };
    let (pack, with) = super_vegeta_override_packs(
        &dir,
        &overrides,
        &fighter,
        "battle-vegeta-001",
        vegeta_palette(None),
    );
    assert_eq!(with.art.objects()[3].id, "battle-vegeta-001@super-vegeta");
    let group = Pose { field: true, fighter: false, ..fighter };
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let mut vegeta = [[0_u8; 3]; 16];
    vegeta[2] = [0, 255, 0];
    let mut super_vegeta = vegeta;
    super_vegeta[14] = yellow;
    super_vegeta[15] = orange;
    let found = |pack: &Arc<Pack>, pose: &Pose, row: &[[u8; 3]; 16]| {
        let art = ArtState::new(Arc::clone(pack));
        let unit = art.unit_for(pose, row);
        art.exact_art(pose, row, unit.as_ref())
    };
    assert_eq!(found(&with, &group, &super_vegeta), (Some(3), false, false), "the picture");
    assert_eq!(found(&with, &group, &vegeta), (None, false, false), "Vegeta's colours: as before");
    assert_eq!(found(&pack, &group, &super_vegeta), (None, false, false), "no folder: as before");
    // Another field group whose key has a picture of `art-override\super-vegeta\` too (`battle-vegeta-804`): it is not
    // Super Vegeta in space, so nothing is found for it, as before
    let second = Object {
        id: "battle-vegeta-804".to_owned(),
        key: super::ENDING_SPACE_VEGETA ^ 0xF0,
        palette: vegeta_palette(None),
        ..object(&group, "art/vegeta.png", false)
    };
    let mut objects = pack.art.objects().to_vec();
    objects.push(second.clone());
    let mut two = (*pack).clone();
    two.art = ArtIndex::new(pack.art.pictures().to_vec(), objects);
    write_png(&overrides.path().join("super-vegeta/battle-vegeta-804.png"), 64, 64, |_, _| {
        [255, 0, 255, 255]
    });
    let two = Arc::new(two.with_art_override(overrides.path()));
    assert!(
        two.art
            .objects_with(second.key)
            .iter()
            .any(|&at| two.art.objects()[at as usize].super_vegeta_only()),
        "the other group's key has a picture"
    );
    let other = Pose { key: second.key, ..group.clone() };
    assert_eq!(found(&two, &other, &super_vegeta), (None, false, false), "another field group");
    assert_eq!(
        found(&two, &group, &super_vegeta).0.map(|at| two.art.objects()[at as usize].id.as_str()),
        Some("battle-vegeta-001@super-vegeta"),
        "with both pictures: his in space"
    );
}

#[test]
fn a_piece_of_a_pose_vegeta_s_art_cannot_draw_in_super_vegeta_s_colours_is_drawn_from_his_picture()
{
    // Super Vegeta vanishing (`battle-vegeta-804`, the stripes of his teleport): the stripes are a piece that the index has
    // no key for, held by Vegeta's art of the whole pose, which cannot be drawn in Super Vegeta's colours (black hair
    // where he has two colours), so the stripes kept the game's sprite even with Ming's picture of the pose in
    // `art-override\super-vegeta\` (found by its key only). The picture of the holder's key is drawn instead; in Vegeta's
    // colours, where his art can draw them, without the folder, or where Vegeta's art draws the piece in Super Vegeta's
    // colours too (a pose that shows no hair), the art is as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (_, pose) = fighter_log(40, 50);
    let (pack, with) = super_vegeta_override_packs(
        &dir,
        &overrides,
        &pose,
        "battle-vegeta-804",
        vegeta_palette(None),
    );
    assert_eq!(with.art.objects()[3].id, "battle-vegeta-804@super-vegeta");
    let piece = Pose { key: pose.key ^ 0xF0, ..pose.clone() };
    let (yellow, orange) = (poses::rgb555(0x03FF), poses::rgb555(0x11DF));
    let mut vegeta = [[0_u8; 3]; 16];
    vegeta[2] = [0, 255, 0];
    let mut super_vegeta = vegeta;
    super_vegeta[14] = yellow;
    super_vegeta[15] = orange;
    let held = |pack: &Arc<Pack>, shaped: bool, row: &[[u8; 3]; 16]| {
        ArtState::new(Arc::clone(pack)).held_override(0, shaped, &piece, &pose, row)
    };
    assert_eq!(held(&with, false, &super_vegeta), Some(3), "held: the picture");
    assert_eq!(held(&with, true, &super_vegeta), Some(3), "same shape: the picture");
    assert_eq!(held(&with, false, &vegeta), None, "Vegeta's colours: his art");
    assert_eq!(held(&pack, false, &super_vegeta), None, "no folder: as before");
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let mut no_hair = vegeta_palette(None);
    no_hair.mask = 1 << 1 | 1 << 2;
    let (_, drawn) =
        super_vegeta_override_packs(&dir, &overrides, &pose, "battle-vegeta-804", no_hair);
    let art = ArtState::new(drawn);
    assert_eq!(
        art.held_override(0, false, &piece, &pose, &super_vegeta),
        None,
        "Vegeta's art draws it: as before"
    );
}

#[test]
fn super_vegeta_s_teleport_stripes_held_by_vegeta_s_art_are_drawn_from_his_picture_in_the_frame() {
    // The frame's side of the test above: Super Vegeta vanishing (`battle-vegeta-804`) is drawn in stripes the index has
    // no key for (in the game a pose of 25 x 39, key 93e8d0529ec1ea88), held by Vegeta's art of the whole pose, which
    // cannot draw them in Super Vegeta's colours. Drawn, the stripes are his picture; in Vegeta's colours they are
    // Vegeta's art, and without the folder they keep the game's sprite, as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut striped, whole) = fighter_log(40, 50);
    let (pack, _) = super_vegeta_override_packs(
        &dir,
        &overrides,
        &whole,
        "battle-vegeta-804",
        vegeta_palette(None),
    );
    // Vegeta's art with the silhouette of the whole 16 x 16 box: what holds the stripes
    let mut objects = pack.art.objects().to_vec();
    objects[0].mask = vec![0xFF; 32];
    let mut held = (*pack).clone();
    held.art = ArtIndex::new(pack.art.pictures().to_vec(), objects);
    let pack = Arc::new(held);
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(with.art.objects()[3].id, "battle-vegeta-804@super-vegeta");
    // Every other column left out: the stripes of the teleport
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            striped.vram[tile * 16 + row] = 0xAA00;
        }
    }
    assert_eq!(drawn_fighter(&with, &striped).1, Some(0), "Vegeta's colours: his art holds them");
    super_vegeta_colours(&mut striped);
    let art = drawn_fighter(&with, &striped).1;
    assert_eq!(
        art.map(|at| with.art.objects()[at as usize].id.as_str()),
        Some("battle-vegeta-804@super-vegeta"),
        "Super Vegeta's colours: his picture"
    );
    assert_eq!(drawn_fighter(&pack, &striped).1, None, "without the folder: the game's sprite");
}

#[test]
fn an_effect_s_pose_found_by_its_shape_is_drawn_from_super_vegeta_s_picture_in_his_palette_of_game_over_only()
 {
    // Vegeta flying over GAME OVER (`gallery-fx-1118`, with the pads): an effect's art (palette mask 0), found by its
    // shape and recoloured from the sprite's own colours, so it draws in any and the picture of `art-override\super-vegeta\`
    // was never drawn in its place. Where the sprite's palette is his palette of GAME OVER (`GAME_OVER_SUPER_VEGETA`,
    // every colour) the picture is drawn, in its own colours; with one colour off (one the pose does not even use), or
    // without the folder, the effect's art, recoloured, as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    write_png(&dir.path().join("art/b.png"), 64, 60, |_, y| match y / 4 + 1 {
        0 => [0, 0, 0, 0],
        1..=5 => [200, 40, 40, 255],
        6..=10 => [40, 200, 40, 255],
        _ => [40, 40, 200, 255],
    });
    write_png(&overrides.path().join("super-vegeta/gallery-fx-1118.png"), 64, 60, |_, _| {
        [255, 0, 255, 255]
    });
    let words = super::GAME_OVER_SUPER_VEGETA;
    let (mut frame, pose) = banded_log(40, 50, [words[1], words[2], words[3]]);
    frame.cgram_start[0x80 + 4 * 16 + 4..0x80 + 4 * 16 + 16].copy_from_slice(&words[4..16]);
    let mut shape = object(&pose, "art/b.png", false);
    shape.id = "gallery-fx-1118".to_owned();
    shape.key ^= 1;
    shape.mask = vec![0; pose.arr.len().div_ceil(8)];
    for n in (0..pose.arr.len()).filter(|&n| pose.arr[n] != 0) {
        shape.mask[n / 8] |= 1 << (n % 8);
    }
    let pack = pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), vec![shape]));
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(with.art.objects()[1].id, "gallery-fx-1118@super-vegeta");
    let drawn = |pack: &Arc<Pack>, frame: &FrameLog| {
        let mut renderer = Renderer::new();
        renderer.set_art(Some(Arc::clone(pack)));
        let drawn = renderer.render_hd(frame, options(4)).unwrap();
        let stats = renderer.art_stats();
        let pose = &stats.poses[0];
        (pose.art, pose.recoloured, pixel(&drawn, 41 * 4 + 1, 53 * 4 + 1), drawn.rgba.clone())
    };
    let (art, recoloured, colour, _) = drawn(&with, &frame);
    assert_eq!(
        (art, recoloured, colour),
        (Some(1), false, [255, 0, 255]),
        "his palette of GAME OVER: the picture, in its own colours"
    );
    let (art, recoloured, _, before) = drawn(&pack, &frame);
    assert_eq!((art, recoloured), (Some(0), true), "without the folder: the effect's art");
    // One colour off (15, which the pose does not use): the effect's art, as without the folder
    frame.cgram_start[0x80 + 4 * 16 + 15] ^= 0x0001;
    let (art, recoloured, _, off) = drawn(&with, &frame);
    assert_eq!((art, recoloured), (Some(0), true), "one colour off: the effect's art");
    assert_eq!(off, drawn(&pack, &frame).3, "one colour off: as without the folder");
    assert_eq!(off, before, "(a colour the pose does not use: the same picture)");
}

#[test]
fn without_super_vegeta_s_folder_every_frame_is_drawn_as_before() {
    // No `art-override\super-vegeta\` (or an empty one): the pack is the same, so every frame is the same to the pixel,
    // in Vegeta's colours, in Super Vegeta's and in any other
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let (pack, _) = super_vegeta_override_packs(
        &dir,
        &overrides,
        &pose,
        "battle-vegeta-803",
        vegeta_palette(None),
    );
    std::fs::remove_file(overrides.path().join("super-vegeta/battle-vegeta-803.png")).unwrap();
    let empty = Arc::new((*pack).clone().with_art_override(overrides.path()));
    let none = Arc::new((*pack).clone().with_art_override(&overrides.path().join("absent")));
    assert_eq!(empty.art.objects(), pack.art.objects());
    assert_eq!(none.art.objects(), pack.art.objects());
    for hair in [None, Some((0x03FF, 0x11DF)), Some((0x03FF, 0x7C00))] {
        if let Some((fourteen, fifteen)) = hair {
            frame.cgram_start[0x80 + 16 + 14] = fourteen;
            frame.cgram_start[0x80 + 16 + 15] = fifteen;
        }
        let before = drawn_fighter(&pack, &frame).3;
        assert_eq!(drawn_fighter(&empty, &frame).3, before, "{hair:?}");
        assert_eq!(drawn_fighter(&none, &frame).3, before, "{hair:?}");
    }
}

/// The pose of [`fighter_log`] as Goku's whole body powering up (patch 0050): the pack has only Super Saiyan art for it
/// (`battle-goku-ssj-020`, red, yellow and gold hair at 14 and 15); the normal Goku's palette (black hair) is known from
/// the art of another pose (`battle-goku-000`).
fn powering_up_pack(dir: &tempfile::TempDir, pose: &Pose) -> Arc<Pack> {
    write_png(&dir.path().join("art/ssj.png"), 64, 64, |_, _| [255, 0, 0, 255]);
    write_png(&dir.path().join("art/normal.png"), 64, 64, |_, _| [0, 0, 255, 255]);
    let goku = |id: &str, key: u64, art: &str, hair: Option<([u8; 3], [u8; 3])>| {
        let mut palette =
            Palette { mask: 1 << 1 | 1 << 2 | 1 << 14 | 1 << 15, ..Palette::default() };
        palette.colors[2] = [0, 255, 0];
        if let Some((fourteen, fifteen)) = hair {
            palette.colors[14] = fourteen;
            palette.colors[15] = fifteen;
        }
        Object { id: id.to_owned(), key, palette, ..object(pose, art, false) }
    };
    let (yellow, gold) = (poses::rgb555(0x53FF), poses::rgb555(0x0A98));
    let objects = vec![
        goku("battle-goku-ssj-020", pose.key, "art/ssj.png", Some((yellow, gold))),
        Object {
            width: 8,
            height: 8,
            ..goku("battle-goku-000", pose.key ^ 1, "art/normal.png", None)
        },
    ];
    pack_of(dir.path(), Vec::new(), ArtIndex::new(Vec::new(), objects))
}

/// Work RAM with `$0C36` = `state` and Goku's party record (slot 0) of unit `unit`.
fn wram_with_goku(state: u8, unit: u8) -> Vec<u8> {
    let mut wram = vec![0_u8; 0x20000];
    wram[0x0C36] = state;
    wram[0x0B00 + 1] = unit;
    wram
}

/// The art drawn for the fighter of [`fighter_log`] from `pack` in `frame` with `conditions`, and its colour.
fn drawn_goku(
    pack: &Arc<Pack>,
    frame: &FrameLog,
    conditions: Conditions,
) -> (Option<u32>, [u8; 3]) {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(pack)));
    renderer.set_conditions(conditions);
    let drawn = renderer.render_hd(frame, options(4)).unwrap();
    let art = renderer.art_stats().poses.iter().find(|p| p.fighter).unwrap().art;
    (art, pixel(&drawn, 41 * 4 + 1, 51 * 4 + 1))
}

#[test]
fn goku_using_the_kaioken_in_a_pose_only_super_saiyan_art_exists_for_keeps_the_game_s_sprite() {
    // "悟空的 N倍界王拳！": the close-up of Goku's whole body powering up, in his own palette (black hair, the outline's
    // colour), with the Kaioken just chosen ($0C36 = $C0, unit $39). The pack has art of that pose for the Super Saiyan
    // only (`battle-goku-ssj-020`): recoloured, it showed him with the Super Saiyan's yellow hair and gold aura. With the
    // Kaioken on, the pose keeps the game's sprite (the green of the frame); without it, the art as before
    let dir = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let pack = powering_up_pack(&dir, &pose);
    let kaioken = Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39));
    assert_eq!(
        drawn_goku(&pack, &frame, kaioken),
        (None, [0, 255, 0]),
        "the Kaioken: the game's sprite"
    );
    let (art, _) = drawn_goku(&pack, &frame, Conditions::default());
    assert_eq!(art, Some(0), "no Kaioken: the Super Saiyan's art, as before");
}

#[test]
fn goku_s_kaioken_with_the_kamehameha_keeps_the_game_s_sprite_in_the_pose_only_super_saiyan_art_exists_for()
 {
    // "界王拳＋龜派氣功" (command $34, unit $3A, the Kaioken on since an earlier turn: $0C36 = $80) draws the same
    // close-up of his whole body powering up in his own palette and in the Kaioken's (the outline and the hair red): the
    // game's sprite, not the Super Saiyan's art
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let pack = powering_up_pack(&dir, &pose);
    let kaioken = Conditions::of_work_ram(&wram_with_goku(0x80, 0x3A));
    assert_eq!(
        drawn_goku(&pack, &frame, kaioken),
        (None, [0, 255, 0]),
        "the Kaioken on: the game's sprite"
    );
    for index in [1, 14, 15] {
        frame.cgram_start[0x80 + 16 + index] = 0x001F; // red
    }
    assert_eq!(drawn_goku(&pack, &frame, kaioken).0, None, "red hair: the game's sprite");
    let (art, _) = drawn_goku(&pack, &frame, Conditions::default());
    assert_eq!(art, Some(0), "red hair without the Kaioken: the art, as before");
}

#[test]
fn super_saiyan_goku_powering_up_keeps_his_art_with_or_without_the_kaioken_bit() {
    // The same close-up in the Super Saiyan palette (Goku turning Super Saiyan, and the Super Saiyan's own moves; a mod
    // that lets him use the Kaioken leaves the hair yellow): his art, whatever $0C36 says
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let pack = powering_up_pack(&dir, &pose);
    frame.cgram_start[0x80 + 16 + 14] = 0x53FF; // yellow
    frame.cgram_start[0x80 + 16 + 15] = 0x0A98; // gold
    for conditions in [
        Conditions::default(),
        Conditions::of_work_ram(&wram_with_goku(0x80, 0x3B)),
        Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39)),
    ] {
        let (art, colour) = drawn_goku(&pack, &frame, conditions);
        assert_eq!((art, colour), (Some(0), [255, 0, 0]), "{conditions:?}: the Super Saiyan's art");
    }
}

#[test]
fn with_the_kaioken_on_super_saiyan_art_of_a_pose_the_normal_goku_also_has_art_for_is_left_to_unit_for()
 {
    // The rule of patch 0050 is for the poses only Super Saiyan art exists for: where the normal Goku has art of his own
    // for the pose, the Super Saiyan's art stays drawable and `unit_for` chooses between the sets (the ending's Goku
    // holding Gohan is the Super Saiyan art whatever the colours, a choice of this branch)
    let dir = tempfile::tempdir().unwrap();
    let (_, pose) = fighter_log(40, 50);
    let mut row = [[0_u8; 3]; 16];
    row[2] = [0, 255, 0];
    let kaioken = Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39));
    let only_super = powering_up_pack(&dir, &pose);
    let mut art = ArtState::new(only_super);
    art.conditions = kaioken;
    assert!(!art.recolourable(&pose, 0, &row), "only the Super Saiyan's art: the game's sprite");
    let (black, yellow, gold) = ([0, 0, 0], poses::rgb555(0x53FF), poses::rgb555(0x0A98));
    let both = goku_pack(
        &dir,
        &pose,
        &[(2, [0, 255, 0]), (14, black), (15, black)],
        &[(2, [0, 255, 0]), (14, yellow), (15, gold)],
    );
    let ssj = both
        .art
        .objects()
        .iter()
        .position(|object| object.id.contains("goku-ssj") && object.key == pose.key)
        .unwrap() as u32;
    let mut art = ArtState::new(both);
    art.conditions = kaioken;
    assert!(art.recolourable(&pose, ssj, &row), "the normal Goku has art too: unit_for decides");
}

/// The pack of [`powering_up_pack`] without and with a folder `overrides` whose `goku\battle-goku-ssj-020.png` (patch 0051)
/// is magenta: the normal Goku's own picture of the pose only Super Saiyan art exists for.
fn normal_goku_override_packs(
    dir: &tempfile::TempDir,
    overrides: &tempfile::TempDir,
    pose: &Pose,
) -> (Arc<Pack>, Arc<Pack>) {
    let pack = powering_up_pack(dir, pose);
    write_png(&overrides.path().join("goku/battle-goku-ssj-020.png"), 64, 64, |_, _| {
        [255, 0, 255, 255]
    });
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    (pack, with)
}

#[test]
fn the_normal_goku_s_own_picture_of_a_pose_only_super_saiyan_art_exists_for_is_drawn_where_his_hair_is_black()
 {
    // `art-override\goku\battle-goku-ssj-020.png`: the whole body powering up with black hair. In the normal Goku's colours
    // (black hair, the outline's colour) it is drawn, with the Kaioken on (its close-up, which kept the game's sprite since
    // patch 0050) and without it (Goku turning Super Saiyan before Frieza, the frames whose palette still has his black
    // hair, drawn from the Super Saiyan art before); the pose of the name stays the Super Saiyan's
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let (pack, with) = normal_goku_override_packs(&dir, &overrides, &pose);
    let added = &with.art.objects()[2];
    assert_eq!(added.id, "battle-goku-ssj-020@goku");
    assert_eq!(
        with.art.objects()[..2],
        pack.art.objects()[..],
        "the Super Saiyan's pose is left as it was"
    );
    let kaioken = Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39));
    for conditions in [kaioken, Conditions::default()] {
        assert_eq!(
            drawn_goku(&with, &frame, conditions),
            (Some(2), [255, 0, 255]),
            "{conditions:?}: the normal Goku's picture"
        );
    }
    assert_eq!(
        drawn_goku(&pack, &frame, kaioken),
        (None, [0, 255, 0]),
        "without the folder: 0050's sprite"
    );
    assert_eq!(
        drawn_goku(&pack, &frame, Conditions::default()).0,
        Some(0),
        "without the folder: the Super Saiyan's art"
    );
}

#[test]
fn the_normal_goku_s_own_picture_is_never_drawn_in_the_super_saiyan_s_hair_and_in_the_kaioken_s_only_with_its_outline()
 {
    // The same pose in the Super Saiyan's colours (yellow and gold hair) is his art, as without the folder, whatever the
    // conditions. In the Kaioken's colours (with the Kamehameha) the picture's black hair and outline are one colour: where
    // the frame has them one colour too (the outline and the hair red) the picture is drawn in it; where it has two (red
    // hair, a black outline) the picture's hair would stay black, so the pose keeps the game's sprite as in patch 0050
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let (pack, with) = normal_goku_override_packs(&dir, &overrides, &pose);
    let mut gold = frame.clone();
    gold.cgram_start[0x80 + 16 + 14] = 0x53FF; // yellow
    gold.cgram_start[0x80 + 16 + 15] = 0x0A98; // gold
    for conditions in [
        Conditions::default(),
        Conditions::of_work_ram(&wram_with_goku(0x80, 0x3B)),
        Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39)),
    ] {
        assert_eq!(
            drawn_goku(&with, &gold, conditions),
            (Some(0), [255, 0, 0]),
            "{conditions:?}: the Super Saiyan's art"
        );
        assert_eq!(drawn_goku(&with, &gold, conditions), drawn_goku(&pack, &gold, conditions));
    }
    let kaioken = Conditions::of_work_ram(&wram_with_goku(0x80, 0x3A));
    let mut red = frame.clone();
    for index in [1, 14, 15] {
        red.cgram_start[0x80 + 16 + index] = 0x001F; // red
    }
    assert_eq!(
        drawn_goku(&with, &red, kaioken).0,
        Some(2),
        "red outline and hair: the picture, in red"
    );
    assert_eq!(
        drawn_goku(&pack, &red, kaioken),
        (None, [0, 255, 0]),
        "without the folder: 0050's sprite"
    );
    let mut red_hair = frame.clone();
    for index in [14, 15] {
        red_hair.cgram_start[0x80 + 16 + index] = 0x001F; // red
    }
    assert_eq!(
        drawn_goku(&with, &red_hair, kaioken),
        (None, [0, 255, 0]),
        "red hair, black outline: the game's sprite"
    );
    assert_eq!(drawn_goku(&with, &red_hair, kaioken), drawn_goku(&pack, &red_hair, kaioken));
}

#[test]
fn the_normal_goku_s_own_picture_is_drawn_in_a_fade_whose_colours_are_nearer_the_super_saiyan_s() {
    // A fade whitens the normal Goku's colours: his outline and black hair become a light grey, nearer the Super Saiyan's
    // yellow than the black of the picture's palette, yet not his hair. The picture is the unit drawn from (`unit_for`),
    // not the Super Saiyan's art the nearest palette would give
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (mut frame, pose) = fighter_log(40, 50);
    let (pack, with) = normal_goku_override_packs(&dir, &overrides, &pose);
    for index in [1, 14, 15] {
        frame.cgram_start[0x80 + 16 + index] = 0x6318; // light grey
    }
    assert_eq!(
        drawn_goku(&with, &frame, Conditions::default()).0,
        Some(2),
        "the normal Goku's picture"
    );
    assert_eq!(
        drawn_goku(&pack, &frame, Conditions::default()).0,
        Some(0),
        "without the folder: the Super Saiyan's art"
    );
}

#[test]
fn without_the_goku_folder_every_frame_is_drawn_as_before() {
    // No `art-override\goku\` (or an empty one): the pack is the same, and so is every frame, in the normal Goku's colours,
    // the Super Saiyan's and the Kaioken's, with and without the Kaioken on
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let (pack, _) = normal_goku_override_packs(&dir, &overrides, &pose);
    std::fs::remove_file(overrides.path().join("goku/battle-goku-ssj-020.png")).unwrap();
    let empty = Arc::new((*pack).clone().with_art_override(overrides.path()));
    let none = Arc::new((*pack).clone().with_art_override(&overrides.path().join("absent")));
    assert_eq!(empty.art.objects(), pack.art.objects());
    assert_eq!(none.art.objects(), pack.art.objects());
    for hair in [None, Some((0x53FF, 0x0A98)), Some((0x001F, 0x001F))] {
        let mut frame = frame.clone();
        if let Some((fourteen, fifteen)) = hair {
            frame.cgram_start[0x80 + 16 + 14] = fourteen;
            frame.cgram_start[0x80 + 16 + 15] = fifteen;
        }
        for conditions in
            [Conditions::default(), Conditions::of_work_ram(&wram_with_goku(0xC0, 0x39))]
        {
            assert_eq!(
                drawn_goku(&empty, &frame, conditions),
                drawn_goku(&pack, &frame, conditions),
                "{hair:?}"
            );
            assert_eq!(
                drawn_goku(&none, &frame, conditions),
                drawn_goku(&pack, &frame, conditions),
                "{hair:?}"
            );
        }
    }
}

/// A fighter (sprite 0, 16 x 16 at (40, 10), OAM priority bits `priority`) over the BG4 layer of
/// [`background_frame`], and a pack with the layer's art and the fighter's: its art is clear on its
/// left half (columns 40..48) and blue on its right half.
fn fighter_over_background(dir: &std::path::Path, priority: u8) -> (FrameLog, Arc<Pack>) {
    let mut log = background_frame(0);
    for entry in &mut log.entries {
        entry.regs.main_obj = true;
    }
    log.oam_low[0..4].copy_from_slice(&[40, 10, 0, 1 << 1 | priority << 4]);
    log.oam_high[0] = 0b10;
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            log.vram[tile * 16 + row] = 0xFF00;
        }
    }
    log.cgram_start[0x80 + 16 + 2] = 0x03E0;
    let sprites = poses::sprites(&log);
    let found: Vec<Pose> =
        poses::poses(&log, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(found.len(), 1);
    let clear_left = |x: u32, _| if x < 32 { [0, 0, 0, 0] } else { [0, 0, 255, 255] };
    write_png(&dir.join("art/o.png"), 64, 64, clear_left);
    let mut pack = Arc::try_unwrap(background_pack(dir, &log.vram, true)).unwrap();
    pack.art = ArtIndex::new(Vec::new(), vec![object(&found[0], "art/o.png", false)]);
    (log, Arc::new(pack))
}

/// The pixel at the middle of game pixel `(x, y)` of the window, drawn with `pack` by the 256 wide
/// renderer and by the wide view (372 wide: the window starts at 58).
fn both_views(pack: &Arc<Pack>, frame: &FrameLog, (x, y): (usize, usize)) -> [[u8; 3]; 2] {
    let mut renderer = Renderer::new();
    renderer.set_art(Some(Arc::clone(pack)));
    let drawn = renderer.render_hd(frame, options(4)).unwrap();
    let mut wide = WideRenderer::new();
    wide.set_art(Some(Arc::clone(pack)));
    let wide_options = WideOptions { width: 372, hud: HudPlacement::Window };
    let wide_drawn = wide.render_hd(Rom(&[]), 0x02, None, frame, wide_options, options(4)).unwrap();
    [pixel(&drawn, x * 4 + 1, y * 4 + 1), pixel(&wide_drawn.picture, (58 + x) * 4 + 1, y * 4 + 1)]
}

#[test]
fn where_a_poses_art_is_clear_the_layer_with_art_shows_though_a_window_could_hide_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut frame, pack) = fighter_over_background(dir.path(), 0);
    // The main screen hides BG4 inside window 1, which is far to the right (columns 200..250):
    // BG4 shows all around the fighter, and the original's sprite hides it where the art is clear
    for entry in &mut frame.entries {
        entry.regs.main_bg_window_off[3] = true;
        entry.regs.bg_window_area[0][3] = 1;
        entry.regs.window_left[0] = 200;
        entry.regs.window_right[0] = 250;
    }
    // Frame row 15 is the layer's row 16; the layer's art is (x, row, 0) at game pixel x
    let clear = both_views(&pack, &frame, (42, 15));
    assert_eq!(clear, [[42, 16, 0]; 2], "the layer's art, not its look");
    assert_eq!(both_views(&pack, &frame, (50, 15)), [[0, 0, 255]; 2], "the fighter's art");
    assert_eq!(both_views(&pack, &frame, (30, 15)), [[30, 16, 0]; 2], "beside the fighter");
    // Inside the window BG4 is hidden: the backdrop
    assert_eq!(both_views(&pack, &frame, (220, 15)), [[0, 0, 0]; 2]);
}

#[test]
fn where_a_poses_art_is_clear_a_layer_without_art_in_front_of_the_background_shows() {
    let dir = tempfile::tempdir().unwrap();
    // The fighter has the highest priority: in front of BG2, whose white tiles (no art) are in
    // front of BG4 over the columns 32..56 and the frame rows 7..31 (an explosion's ground)
    let (mut frame, pack) = fighter_over_background(dir.path(), 3);
    for entry in &mut frame.entries {
        entry.regs.main_bg[1] = true;
        entry.regs.bg_map_base[1] = 0x0800;
        entry.regs.bg_char_base[1] = 0x2000;
    }
    for word in 0..8 {
        frame.vram[0x2000 + 8 + word] = 0x00FF;
    }
    for row in 1..4 {
        for column in 4..7 {
            frame.vram[0x0800 + row * 32 + column] = 1;
        }
    }
    frame.cgram_start[0x21] = 0x7FFF;
    assert_eq!(both_views(&pack, &frame, (42, 15)), [[255; 3]; 2], "BG2's white, not BG4's art");
    assert_eq!(both_views(&pack, &frame, (50, 15)), [[0, 0, 255]; 2], "the fighter's art in front");
    assert_eq!(both_views(&pack, &frame, (34, 15)), [[255; 3]; 2], "BG2 beside the fighter");
    assert_eq!(both_views(&pack, &frame, (60, 15)), [[60, 16, 0]; 2], "BG4's art past BG2");
}

#[test]
fn the_record_under_a_pose_is_taken_for_its_hidden_sprites_pixels_alone() {
    use crate::bgart::Kind;
    let width = 8;
    let mut pixels = vec![super::ArtPixel::NONE; width * 4];
    let mut under = vec![super::ArtPixel::NONE; width * 4];
    for pixel in &mut under {
        pixel.bg.kind = Kind::Layer;
        pixel.bg.shadow = 7;
    }
    // Row 1: a pixel of sprite 3 (hidden), of sprite 4 (not hidden) and of a layer; row 3: sprite 3
    // again, past the rows the poses reach
    for (at, oam) in [(width + 1, 3), (width + 2, 4), (3 * width + 1, 3)] {
        pixels[at].oam = oam;
        pixels[at].prio = 5;
        pixels[at].bg.kind = Kind::Object;
    }
    super::merge_under(&mut pixels, &under, width, 1 << 3, Some((1, 3)));
    assert_eq!((pixels[width + 1].bg.kind, pixels[width + 1].bg.shadow), (Kind::Layer, 7));
    assert_eq!((pixels[width + 1].oam, pixels[width + 1].prio), (3, 5), "the sprite's own");
    assert_eq!(pixels[width + 2].bg.kind, Kind::Object, "a sprite that is not hidden");
    assert_eq!(pixels[width + 3].bg.kind, Kind::Hidden, "a layer's pixel");
    assert_eq!(pixels[3 * width + 1].bg.kind, Kind::Object, "a row the poses do not reach");
    let before = pixels.iter().map(|pixel| pixel.bg.kind).collect::<Vec<_>>();
    super::merge_under(&mut pixels, &under, width, 1 << 3, None);
    assert_eq!(pixels.iter().map(|pixel| pixel.bg.kind).collect::<Vec<_>>(), before, "no poses");
}

/// One frame where a pose variant's margin runs over blank text cells
/// behind the fighter (the HD text leaves those pixels to the art, `ArtState::uncover`) and the
/// variant's art is clear over a layer with art that a window hides elsewhere (the layer is drawn
/// as it is with the sprite gone, `merge_under`). The fighter (16 x 16 at (40, 16), priority 0)
/// stands on BG4 of [`background_frame`] (mode 0), under BG3 spaces in white on tile row 1 (frame
/// rows 8..16, where the variant's top margin is); the variant (80 x 80, margin 2) and the pose's
/// art are clear over the sprite's left half (columns 40..48) and magenta / blue elsewhere.
#[test]
fn a_variants_margin_over_text_cells_and_its_clear_art_over_a_layer_hold_in_one_frame() {
    let dir = tempfile::tempdir().unwrap();
    let mut frame = background_frame(0);
    for entry in &mut frame.entries {
        entry.regs.main_obj = true;
        entry.regs.main_bg[2] = true;
        entry.regs.bg_map_base[2] = 0x0800;
        entry.regs.bg_char_base[2] = 0x2000;
        // BG4 hidden inside window 1, far to the right
        entry.regs.main_bg_window_off[3] = true;
        entry.regs.bg_window_area[0][3] = 1;
        entry.regs.window_left[0] = 200;
        entry.regs.window_right[0] = 250;
    }
    frame.oam_low[0..4].copy_from_slice(&[40, 16, 0, 1 << 1]);
    frame.oam_high[0] = 0b10;
    for tile in [0_usize, 1, 16, 17] {
        for row in 0..8 {
            frame.vram[tile * 16 + row] = 0xFF00;
        }
    }
    frame.cgram_start[0x80 + 16 + 2] = 0x03E0;
    // BG3: the font's space (tile 1, colour 1 on every pixel) on tile row 1 only, in white
    for row in 0..8 {
        frame.vram[0x2000 + 8 + row] = 0x00FF;
    }
    frame.vram[0x0800 + 32..0x0800 + 64].fill(1);
    // (mode 0: BG3's colours are 64..96)
    frame.cgram_start[0x41] = 0x7FFF;
    let sprites = poses::sprites(&frame);
    let found: Vec<Pose> =
        poses::poses(&frame, &sprites).into_iter().filter(|pose| !pose.field).collect();
    assert_eq!(found.len(), 1);
    let clear_left = |x: u32, _| if x < 32 { [0, 0, 0, 0] } else { [0, 0, 255, 255] };
    write_png(&dir.path().join("art/o.png"), 64, 64, clear_left);
    let variant = dir.path().join("art/v.png");
    // The variant's box starts 2 game pixels (8 art pixels) in: clear over columns 40..48, rows 16..32
    write_png(&variant, 80, 80, |x, y| {
        if (8..40).contains(&x) && (8..72).contains(&y) { [0, 0, 0, 0] } else { [255, 0, 255, 255] }
    });
    let mut pack = Arc::try_unwrap(background_pack(dir.path(), &frame.vram, true)).unwrap();
    pack.art = ArtIndex::new(Vec::new(), vec![object(&found[0], "art/o.png", false)]);
    pack.variants = vec![PoseVariant {
        art: "battle-x-000".to_owned(),
        when: ssd_mod::pose::Condition::Always,
        margin: 2,
        file: variant.to_string_lossy().into_owned(),
    }];
    let pack = Arc::new(pack);
    let mut renderer = Renderer::new();
    renderer.set_font(Some(space_font()));
    renderer.set_art(Some(Arc::clone(&pack)));
    let drawn = renderer.render_hd(&frame, options(4)).unwrap();
    let mut wide = WideRenderer::new();
    wide.set_font(Some(space_font()));
    wide.set_art(Some(pack));
    let wide_options = WideOptions { width: 372, hud: HudPlacement::Window };
    let wide_drawn =
        wide.render_hd(Rom(&[]), 0x02, None, &frame, wide_options, options(4)).unwrap();
    let both = |x: usize, y: usize| {
        [
            pixel(&drawn, x * 4 + 1, y * 4 + 1),
            pixel(&wide_drawn.picture, (58 + x) * 4 + 1, y * 4 + 1),
        ]
    };
    // (magenta: BG4's look is red)
    let magenta = [255, 0, 255];
    // ssj-hair-fix: the variant's margin over the spaces is the variant's, beside it the cells' white
    assert_eq!(both(45, 14), [magenta; 2], "the margin above the sprite, on a space");
    assert_eq!(both(38, 15), [magenta; 2], "the margin's first column, on a space");
    assert_eq!(both(37, 14), [[0xFF; 3]; 2], "past the margin the cells keep their white");
    // halo-fix: where the variant is clear, BG4's art (a layer the window hides elsewhere)
    assert_eq!(both(42, 20), [[42, 21, 0]; 2], "the layer's art under the clear variant");
    assert_eq!(both(50, 20), [magenta; 2], "the variant over the sprite's right half");
    assert_eq!(both(30, 20), [[30, 21, 0]; 2], "beside the fighter");
}

/// The bytes of the files of background entry `at` of `pack` (its art and its source).
fn bg_file_bytes(pack: &Pack, at: usize) -> u64 {
    let entry = &pack.bg.layer[at];
    [&entry.art, &entry.source]
        .into_iter()
        .filter(|file| !file.is_empty())
        .map(|file| std::fs::metadata(pack.dir.join(file)).unwrap().len())
        .sum()
}

#[test]
fn the_cost_counts_the_background_art_asked_for_received_and_evicted() {
    let dir = tempfile::tempdir().unwrap();
    let mut pack = Arc::try_unwrap(background_pack(dir.path(), &vec![0; 0x8000], false)).unwrap();
    let entry = pack.bg.layer[0].clone();
    pack.bg.layer =
        (0..5).map(|n| crate::bgart::Entry { id: format!("test-{n}"), ..entry.clone() }).collect();
    let pack = Arc::new(pack);
    let files = bg_file_bytes(&pack, 0);
    let mut art = ArtState::new(Arc::clone(&pack));
    art.begin(256, 224, 4);
    for at in 0..5 {
        assert!(art.bg_image(at).is_some(), "entry {at} has its art");
    }
    let cost = art.cost();
    assert_eq!(
        (cost.requests, cost.bg_loads, cost.received()),
        (5, 5, 5),
        "five files asked for and read"
    );
    assert_eq!(cost.load_bytes, 5 * files, "the bytes of the files read");
    assert!(cost.loads > std::time::Duration::ZERO, "{cost:?}");
    assert_eq!(cost.bg_evicted, 0, "five layers are within the budget");
    art.bg_image(4);
    assert_eq!(art.cost().requests, 5, "the one kept is not asked for again");
    // Past a budget the layer used longest ago goes, and the frame's cost counts it
    let layer = art.bg_image(0);
    art.bg_images.insert(9, layer.clone(), 1);
    art.bg_images.insert(8, layer, 1);
    assert_eq!(art.cost().bg_evicted, 6, "the five, then the ninth");
    art.begin(256, 224, 4);
    assert_eq!(art.cost(), crate::DrawCost::default(), "a frame starts its cost over");
}

#[test]
fn the_cost_of_art_read_on_its_own_thread_is_its_asking_and_receiving_not_its_reading() {
    let dir = tempfile::tempdir().unwrap();
    let pack = background_pack(dir.path(), &vec![0; 0x8000], false);
    let files = bg_file_bytes(&pack, 0);
    let mut art = ArtState::new(pack);
    art.set_async(true);
    if !art.loads_async() {
        // (SSD_HD_SERIAL keeps the art read by the frames)
        return;
    }
    art.begin(256, 224, 4);
    assert!(art.bg_image(0).is_none(), "asked of the thread, not read");
    assert!(art.bg_image(0).is_none());
    let cost = art.cost();
    assert_eq!(
        (cost.requests, cost.received(), cost.load_bytes),
        (1, 0, 0),
        "asked once a frame: {cost:?}"
    );
    assert!(art.wait_read(), "the thread reads it");
    art.begin(256, 224, 4);
    let cost = art.cost();
    assert_eq!(
        (cost.requests, cost.bg_loads, cost.received()),
        (0, 1, 1),
        "received when a frame starts"
    );
    assert_eq!(cost.load_bytes, files, "with the bytes of its files");
    assert_eq!(
        cost.loads,
        std::time::Duration::ZERO,
        "the thread's reading is not the frame's time"
    );
    assert!(art.bg_image(0).is_some(), "and drawn");
    assert_eq!(art.cost().requests, 0, "not asked for again");
}

/// The work RAM of a frame of the ending's loop: game mode `mode`, the loop's frame `frames` (`$73`), who is in the ending
/// `party` (`$82`), and Super Saiyan Goku's unit in a party record, as in the ending.
fn wram_of_ending(mode: u8, frames: u16, party: u8) -> Vec<u8> {
    let mut wram = vec![0_u8; 0x20000];
    wram[0x0024] = mode;
    wram[0x0073..0x0075].copy_from_slice(&frames.to_le_bytes());
    wram[0x0082] = party;
    wram[0x0B00 + 1] = 0x3B;
    wram
}

#[test]
fn the_ending_flight_is_mode_11_until_goku_crouches_before_vegeta() {
    // `00:B0EE` draws Goku from the flight's sprites while `$73` is under `$1F40`, and from the crouch's from then on with
    // Vegeta in the ending (`$82` bit 7); without him Goku flies on. Super Saiyan Goku's unit in a party record (outside
    // the ending too, in every fight after Frieza's) is not the flight
    let flight = |mode, frames, party| {
        Conditions::of_work_ram(&wram_of_ending(mode, frames, party)).ending_flight
    };
    assert!(flight(0x11, 0x0080, 0x87), "the first frame of the flight");
    assert!(flight(0x11, 0x1F3F, 0x87), "its last frame with Vegeta");
    assert!(!flight(0x11, 0x1F40, 0x87), "Goku crouching with Gohan before Vegeta");
    assert!(!flight(0x11, 0x290C, 0x87), "Vegeta's speech");
    assert!(flight(0x11, 0x1F40, 0x07), "without Vegeta he flies on");
    assert!(flight(0x91, 0x0100, 0x87), "bit 7 of the mode aside");
    for mode in [0x00, 0x02, 0x04, 0x07, 0x0C, 0x12] {
        assert!(!flight(mode, 0x0100, 0x87), "mode {mode:02X} is not the flight");
    }
    assert!(!Conditions::of_work_ram(&[0x11; 0x40]).ending_flight, "too short: no");
    assert!(!Conditions::default().ending_flight);
}

#[test]
fn goku_s_picture_of_the_ending_folder_is_drawn_in_the_ending_flight_only() {
    // `art-override\ending-goku\battle-goku-000.png` (magenta): Goku in torn clothes, drawn in place of the normal art of
    // its name while the ending's flight is on screen; in a fight (the same art, before Namek or with `$3B` after Frieza),
    // with Goku crouching before Vegeta and without the folder, the art as before
    let dir = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let (frame, pose) = fighter_log(40, 50);
    let pack = goku_pack(
        &dir,
        &pose,
        &[(2, [0, 255, 0]), (3, [0, 0, 0])],
        &[(2, [255, 198, 0]), (3, [0, 0, 0])],
    );
    std::fs::create_dir_all(overrides.path().join("ending-goku")).unwrap();
    let empty = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(empty.art.objects(), pack.art.objects(), "an empty folder adds nothing");
    write_png(&overrides.path().join("ending-goku/battle-goku-000.png"), 64, 64, |_, _| {
        [255, 0, 255, 255]
    });
    let with = Arc::new((*pack).clone().with_art_override(overrides.path()));
    assert_eq!(
        with.art.objects()[..3],
        pack.art.objects()[..],
        "the art of the name is left as it was"
    );
    assert_eq!(with.art.objects()[3].id, "battle-goku-000@ending-goku");
    let flight = Conditions::of_work_ram(&wram_of_ending(0x11, 0x0100, 0x87));
    assert_eq!(
        drawn_goku(&with, &frame, flight),
        (Some(3), [255, 0, 255]),
        "the flight: the folder's picture"
    );
    assert_eq!(drawn_goku(&pack, &frame, flight).0, Some(0), "without the folder: the normal art");
    for (conditions, what) in [
        (Conditions::default(), "a fight before Namek"),
        (
            Conditions::of_work_ram(&wram_of_ending(0x02, 0x0100, 0x87)),
            "a fight with $3B in the party",
        ),
        (Conditions::of_work_ram(&wram_of_ending(0x11, 0x1F40, 0x87)), "crouching before Vegeta"),
    ] {
        assert_eq!(drawn_goku(&with, &frame, conditions).0, Some(0), "{what}: the normal art");
        assert_eq!(
            drawn_goku(&with, &frame, conditions),
            drawn_goku(&pack, &frame, conditions),
            "{what}: as without the folder"
        );
    }
}
