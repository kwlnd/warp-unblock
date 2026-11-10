//! Светящаяся сетка за главной карточкой. Неподвижная часть (тусклая сетка и ровное свечение
//! «подключено») запекается в картинку один раз на размер карточки, цвет и масштаб экрана;
//! каждый кадр рисуются картинка и только живые клетки: пятна, волны, «звёзды».
use gpui_kit::*;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// Простой: плывут мягкие пятна.
    Idle,
    /// Подключение: резкие волны, каждая в случайном направлении.
    Waves,
    /// Подключено: сетка ровно светится, местами подмигивают «звёзды».
    On,
    /// Ошибка: пятна цвета ошибки.
    Error,
}

const STEP: f32 = 7.;
const CELL: f32 = 4.;
const RADIUS: f32 = 14.;
/// Яркость запечённой сетки «подключено».
const LIT: f32 = 0.52;
/// Живые клетки слабее этого порога не рисуем — их хватает запечённой сетки.
const VISIBLE: f32 = 0.03;

struct Cell {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    seed: f32,
}

#[derive(Default)]
pub struct Cache {
    key: Option<(u32, u32, u32, u32)>,
    cells: Vec<Cell>,
    base: Option<Arc<RenderImage>>,
    lit: Option<Arc<RenderImage>>,
}

/// Детерминированный шум 0..1 по двум координатам: направления волн и «звёзды».
pub fn noise(x: i32, y: i32, salt: u32) -> f32 {
    let mut n = (x as u32)
        .wrapping_mul(0x27d4_eb2d)
        .wrapping_add((y as u32).wrapping_mul(0x1656_67b1))
        .wrapping_add(salt.wrapping_mul(0x9e37_79b9));
    n ^= n >> 15;
    n = n.wrapping_mul(0x85eb_ca6b);
    n ^= n >> 13;
    n = n.wrapping_mul(0xc2b2_ae35);
    n ^= n >> 16;
    n as f32 / u32::MAX as f32
}

fn pack(color: Hsla) -> u32 {
    let c = Rgba::from(color);
    u32::from_be_bytes([
        (c.r * 255.) as u8,
        (c.g * 255.) as u8,
        (c.b * 255.) as u8,
        (c.a * 255.) as u8,
    ])
}

/// Клетки сетки по центру карточки; за скруглёнными углами клеток нет.
fn layout(width: f32, height: f32) -> Vec<Cell> {
    let cols = (width / STEP).floor() as i32;
    let rows = (height / STEP).floor() as i32;
    let ox = (width - (cols as f32 - 1.) * STEP - CELL) / 2.;
    let oy = (height - (rows as f32 - 1.) * STEP - CELL) / 2.;
    let mut cells = Vec::with_capacity((cols.max(0) * rows.max(0)) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let (x, y) = (ox + col as f32 * STEP, oy + row as f32 * STEP);
            let (cx, cy) = (x + CELL / 2., y + CELL / 2.);
            let qx = (RADIUS - cx).max(cx - (width - RADIUS)).max(0.);
            let qy = (RADIUS - cy).max(cy - (height - RADIUS)).max(0.);
            if qx * qx + qy * qy > (RADIUS - 3.) * (RADIUS - 3.) {
                continue;
            }
            cells.push(Cell {
                x,
                y,
                u: cx / width,
                v: cy / height,
                seed: noise(col, row, 3),
            });
        }
    }
    cells
}

/// Запекает сетку одного цвета и яркости в картинку размером карточки в пикселях экрана.
fn bake(
    cells: &[Cell],
    width: u32,
    height: u32,
    scale: f32,
    color: Hsla,
    alpha: f32,
) -> Arc<RenderImage> {
    let c = Rgba::from(color);
    let a = (alpha * c.a).clamp(0., 1.);
    // BGRA, как ждёт RenderImage; цвет домножен на альфу.
    let pixel = [
        (c.b * a * 255.) as u8,
        (c.g * a * 255.) as u8,
        (c.r * a * 255.) as u8,
        (a * 255.) as u8,
    ];
    let mut data = vec![0u8; (width * height * 4) as usize];
    for cell in cells {
        let x0 = (cell.x * scale).round() as u32;
        let y0 = (cell.y * scale).round() as u32;
        let x1 = (((cell.x + CELL) * scale).round() as u32).min(width);
        let y1 = (((cell.y + CELL) * scale).round() as u32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * width + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&pixel);
            }
        }
    }
    let buffer = image::RgbaImage::from_raw(width, height, data).expect("glow buffer");
    Arc::new(RenderImage::new([image::Frame::new(buffer)]))
}

pub struct Frame {
    pub mode: Mode,
    pub color: Hsla,
    pub star: Hsla,
    pub floor: f32,
    pub time: f32,
}

pub fn paint(window: &mut Window, bounds: Bounds<Pixels>, cache: &mut Cache, frame: &Frame) {
    let scale = window.scale_factor();
    let width = bounds.size.width / px(1.);
    let height = bounds.size.height / px(1.);
    if width < 1. || height < 1. {
        return;
    }
    let (wd, hd) = (
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    );
    let key = (wd, hd, pack(frame.color), frame.floor.to_bits());
    if cache.key != Some(key) {
        for image in [cache.base.take(), cache.lit.take()].into_iter().flatten() {
            let _ = window.drop_image(image);
        }
        cache.cells = layout(width, height);
        cache.base = Some(bake(&cache.cells, wd, hd, scale, frame.color, frame.floor));
        cache.lit = Some(bake(&cache.cells, wd, hd, scale, frame.color, LIT));
        cache.key = Some(key);
    }
    let corners = Corners::all(px(RADIUS));
    let baked = if frame.mode == Mode::On {
        &cache.lit
    } else {
        &cache.base
    };
    if let Some(image) = baked {
        let _ = window.paint_image(bounds, bounds, corners, image.clone(), 0, false);
    }

    let t = frame.time;
    let scale_min = width.min(height).max(1.);
    let blobs = [
        (
            0.5 + 0.34 * (t * 0.37).sin(),
            0.38 + 0.22 * (t * 0.53 + 1.0).sin(),
            0.3,
            1.0,
        ),
        (
            0.5 + 0.38 * (t * 0.29 + 2.0).cos(),
            0.66 + 0.2 * (t * 0.41).cos(),
            0.24,
            0.9,
        ),
        (
            0.5 + 0.25 * (t * 0.19).cos(),
            0.5 + 0.25 * (t * 0.19).sin(),
            0.16,
            0.55,
        ),
    ];
    let period = 0.9f32;
    let wave = |offset: f32| {
        let n = ((t + offset) / period).floor();
        let phase = ((t + offset) / period).fract();
        let angle = noise(n as i32, 7, 11) * std::f32::consts::TAU;
        (angle.cos(), angle.sin(), -0.25 + 1.5 * phase)
    };
    let waves = [wave(0.0), wave(period * 0.5)];
    if frame.mode == Mode::On {
        // Ровное свечение «дышит» одним полупрозрачным слоем поверх запечённой сетки.
        let breathe = 0.04 * (1. + (t * 1.1).sin());
        window.paint_quad(fill(bounds, frame.color.opacity(breathe)).corner_radii(corners));
    }
    for cell in &cache.cells {
        let (fill_color, alpha) = match frame.mode {
            Mode::Idle | Mode::Error => {
                let mut glow = 0.0f32;
                for (bx, by, r, w) in blobs {
                    let dx = (cell.x - bx * width) / (r * scale_min);
                    let dy = (cell.y - by * height) / (r * scale_min);
                    glow += w * (-(dx * dx + dy * dy)).exp();
                }
                let strength = if frame.mode == Mode::Idle { 0.8 } else { 1.0 };
                (frame.color, strength * glow.min(1.).powf(1.5))
            }
            Mode::Waves => {
                let mut glow = 0.0f32;
                for (dx, dy, front) in waves {
                    let along = ((cell.u - 0.5) * dx + (cell.v - 0.5) * dy) / 1.42 + 0.5;
                    let d = along - front;
                    // Резкий передний край и короткий светящийся хвост.
                    glow += if d > 0. {
                        (-(d / 0.012).powi(2)).exp()
                    } else {
                        (d / 0.09).exp()
                    };
                }
                (frame.color, 0.95 * glow.min(1.))
            }
            Mode::On => {
                if cell.seed >= 0.07 {
                    continue;
                }
                let twinkle = ((t * (1.2 + cell.seed * 20.) + cell.seed * 97.)
                    .sin()
                    .max(0.))
                .powf(10.);
                (
                    frame.star,
                    (0.4 + 0.6 * twinkle) * (twinkle > 0.02) as u8 as f32,
                )
            }
        };
        if alpha < VISIBLE {
            continue;
        }
        window.paint_quad(
            fill(
                Bounds::new(
                    point(bounds.origin.x + px(cell.x), bounds.origin.y + px(cell.y)),
                    size(px(CELL), px(CELL)),
                ),
                fill_color.opacity(alpha.min(1.)),
            )
            .corner_radii(Corners::all(px(1.))),
        );
    }
}
