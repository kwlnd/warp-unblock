//! Светящаяся сетка за главной карточкой. Неподвижная часть (тусклая сетка и ровное свечение
//! «подключено») запекается в картинку один раз на размер карточки, цвет и масштаб экрана;
//! каждый кадр рисуются мягкое свечение (размытые тени), картинка и только живые клетки:
//! пятна, волны, «звёзды».
use gpui_kit::*;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// Простой: плывут мягкие пятна.
    Idle,
    /// Подключение: светящиеся волны, каждая в случайном направлении.
    Waves,
    /// Подключено: сетка ровно светится, местами подмигивают «звёзды».
    On,
    /// Ошибка: пятна цвета ошибки.
    Error,
}

const STEP: f32 = 7.;
const CELL: f32 = 4.;
const RADIUS: f32 = 14.;
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

#[derive(Clone)]
pub struct Frame {
    pub mode: Mode,
    /// Цвет живых клеток и свечения.
    pub color: Hsla,
    /// Самые яркие клетки: гребень волны, середина пятна, «звёзды».
    pub hot: Hsla,
    /// Цвет неподвижной тусклой сетки.
    pub base: Hsla,
    /// Яркость неподвижной сетки.
    pub floor: f32,
    /// Яркость запечённой сетки «подключено».
    pub lit: f32,
    /// Сила мягкого свечения под клетками.
    pub haze: f32,
    /// Предел яркости живых клеток.
    pub peak: f32,
    /// Доля слоя при смене состояния: 1 — виден целиком. Прозрачность элемента GPUI
    /// к canvas не применяется, поэтому гасим сами.
    pub weight: f32,
    pub time: f32,
}

/// Сколько секунд прежнее состояние гаснет, а новое проявляется.
const FADE: f32 = 0.7;

/// Две сетки на время смены состояния: у каждой своя запечённая картинка.
#[derive(Default)]
pub struct State {
    pub current: Cache,
    pub previous: Cache,
    last: Option<Frame>,
    from: Option<Frame>,
    since: f32,
}

impl State {
    /// Запоминает кадр. Пока идёт смена состояния, возвращает прежний кадр и долю нового (0..1).
    pub fn advance(&mut self, frame: &Frame) -> Option<(Frame, f32)> {
        if let Some(last) = &self.last {
            if !last.same_look(frame) {
                let k = (frame.time - self.since) / FADE;
                // Промежуточное состояние едва проявилось — просто меняем цель перехода,
                // иначе прежняя сетка исчезла бы скачком.
                if self.from.is_none() || k >= 0.5 {
                    self.from = self.last.take();
                    self.since = frame.time;
                    std::mem::swap(&mut self.current, &mut self.previous);
                }
            }
        }
        self.last = Some(frame.clone());
        let k = ((frame.time - self.since) / FADE).clamp(0., 1.);
        if k >= 1. {
            self.from = None;
        }
        let from = self.from.as_ref()?;
        let k = k * k * (3. - 2. * k);
        Some((
            Frame {
                time: frame.time,
                weight: 1. - k,
                ..from.clone()
            },
            k,
        ))
    }
}

impl Frame {
    fn same_look(&self, other: &Frame) -> bool {
        self.mode == other.mode
            && self.color == other.color
            && self.hot == other.hot
            && self.base == other.base
            && self.floor == other.floor
            && self.lit == other.lit
            && self.haze == other.haze
            && self.peak == other.peak
    }
}

/// Мягкое круглое свечение радиусом `radius`: размытая тень без дырки посередине.
fn bloom(window: &mut Window, origin: Point<Pixels>, x: f32, y: f32, radius: f32, color: Hsla) {
    if color.a < 0.005 {
        return;
    }
    let r = radius * 0.45;
    window.paint_drop_shadows(
        Bounds::new(
            point(origin.x + px(x - r), origin.y + px(y - r)),
            size(px(2. * r), px(2. * r)),
        ),
        Corners::all(px(r)),
        &[BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(r),
            spread_radius: px(0.),
            inset: false,
        }],
    );
}

fn mix(a: Hsla, b: Hsla, k: f32) -> Hsla {
    let k = k.clamp(0., 1.);
    Hsla {
        h: a.h + (b.h - a.h) * k,
        s: a.s + (b.s - a.s) * k,
        l: a.l + (b.l - a.l) * k,
        a: a.a + (b.a - a.a) * k,
    }
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
    let key = (
        wd,
        hd,
        pack(frame.color) ^ pack(frame.base).rotate_left(7),
        frame.floor.to_bits() ^ frame.lit.to_bits().rotate_left(11),
    );
    if cache.key != Some(key) {
        for image in [cache.base.take(), cache.lit.take()].into_iter().flatten() {
            let _ = window.drop_image(image);
        }
        cache.cells = layout(width, height);
        cache.base = Some(bake(&cache.cells, wd, hd, scale, frame.base, frame.floor));
        cache.lit = Some(bake(&cache.cells, wd, hd, scale, frame.color, frame.lit));
        cache.key = Some(key);
    }
    let corners = Corners::all(px(RADIUS));
    let origin = bounds.origin;
    let t = frame.time;
    let scale_min = width.min(height).max(1.);
    let w = frame.weight.clamp(0., 1.);
    let haze = frame.haze * w;

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
    let period = 1.1f32;
    let wave = |offset: f32| {
        let n = ((t + offset) / period).floor();
        let phase = ((t + offset) / period).fract();
        let angle = noise(n as i32, 7, 11) * std::f32::consts::TAU;
        (angle.cos(), angle.sin(), -0.25 + 1.5 * phase)
    };
    let waves = [wave(0.0), wave(period * 0.5)];

    // Свечение лежит под сеткой: клетки поверх него читаются как светящиеся точки.
    match frame.mode {
        Mode::Idle | Mode::Error => {
            for (bx, by, r, strength) in blobs {
                bloom(
                    window,
                    origin,
                    bx * width,
                    by * height,
                    r * scale_min * 2.,
                    frame.color.opacity(haze * strength),
                );
            }
        }
        Mode::Waves => {
            let tint = mix(frame.color, frame.hot, 0.5);
            for (dx, dy, front) in waves {
                // Гребень — прямая поперёк направления волны; вдоль неё ряд мягких пятен.
                let along = (front - 0.5) * 1.42 - 0.03;
                let (cx, cy) = (0.5 + along * dx, 0.5 + along * dy);
                let fade = (1. - ((front - 0.5) / 0.8).abs()).clamp(0., 1.).sqrt();
                for k in -4..=4 {
                    let s = k as f32 * 0.2;
                    let (x, y) = ((cx - dy * s) * width, (cy + dx * s) * height);
                    if x < -80. || y < -80. || x > width + 80. || y > height + 80. {
                        continue;
                    }
                    bloom(
                        window,
                        origin,
                        x,
                        y,
                        scale_min * 0.4,
                        tint.opacity(haze * fade),
                    );
                }
            }
        }
        Mode::On => {
            let breathe = 0.75 + 0.25 * (t * 1.1).sin();
            bloom(
                window,
                origin,
                width * 0.5,
                height * 0.36,
                scale_min * 1.2,
                frame.color.opacity(haze * breathe),
            );
        }
    }

    let (baked, baked_color, baked_alpha) = if frame.mode == Mode::On {
        (&cache.lit, frame.color, frame.lit)
    } else {
        (&cache.base, frame.base, frame.floor)
    };
    if w >= 0.999 {
        if let Some(image) = baked {
            let _ = window.paint_image(bounds, bounds, corners, image.clone(), 0, false);
        }
    } else {
        // Картинку GPUI не умеет рисовать полупрозрачной: на время перехода сетка — квадами.
        let color = baked_color.opacity(baked_alpha * baked_color.a * w);
        for cell in &cache.cells {
            window.paint_quad(fill(
                Bounds::new(
                    point(origin.x + px(cell.x), origin.y + px(cell.y)),
                    size(px(CELL), px(CELL)),
                ),
                color,
            ));
        }
    }

    for cell in &cache.cells {
        let (fill_color, alpha) = match frame.mode {
            Mode::Idle | Mode::Error => {
                let mut glow = 0.0f32;
                for (bx, by, r, blob) in blobs {
                    let dx = (cell.x - bx * width) / (r * scale_min);
                    let dy = (cell.y - by * height) / (r * scale_min);
                    glow += blob * (-(dx * dx + dy * dy)).exp();
                }
                let glow = glow.min(1.).powf(1.5);
                let strength = if frame.mode == Mode::Idle { 0.8 } else { 1.0 };
                // Середина пятна теплее и ярче краёв.
                (
                    mix(frame.color, frame.hot, glow * glow * 0.6),
                    strength * glow,
                )
            }
            Mode::Waves => {
                let mut glow = 0.0f32;
                let mut crest = 0.0f32;
                for (dx, dy, front) in waves {
                    let along = ((cell.u - 0.5) * dx + (cell.v - 0.5) * dy) / 1.42 + 0.5;
                    let d = along - front;
                    // Мягкий передний край, яркий гребень и длинный мерцающий хвост.
                    let edge = (-(d / 0.022).powi(2)).exp();
                    let tail = if d > 0. { 0. } else { (d / 0.11).exp() };
                    let sparkle = 0.65 + 0.35 * (t * 7. + cell.seed * 60.).sin();
                    glow += edge.max(tail * sparkle);
                    crest = crest.max(edge);
                }
                (mix(frame.color, frame.hot, crest), glow.min(1.))
            }
            Mode::On => {
                if cell.seed >= 0.07 {
                    continue;
                }
                let twinkle = ((t * (1.2 + cell.seed * 20.) + cell.seed * 97.)
                    .sin()
                    .max(0.))
                .powf(10.);
                // У самого края ореол залез бы на рамку карточки.
                let edge = cell.x.min(cell.y).min(width - cell.x).min(height - cell.y);
                if twinkle <= 0.02 || edge < 12. {
                    continue;
                }
                // Яркая «звезда» с мягким ореолом.
                bloom(
                    window,
                    origin,
                    cell.x + CELL / 2.,
                    cell.y + CELL / 2.,
                    14.,
                    frame.hot.opacity(haze * 1.3 * twinkle),
                );
                (frame.hot, 0.4 + 0.6 * twinkle)
            }
        };
        let alpha = alpha * frame.peak * w;
        if alpha < VISIBLE {
            continue;
        }
        window.paint_quad(
            fill(
                Bounds::new(
                    point(origin.x + px(cell.x), origin.y + px(cell.y)),
                    size(px(CELL), px(CELL)),
                ),
                fill_color.opacity(alpha.min(1.)),
            )
            .corner_radii(Corners::all(px(1.))),
        );
    }
}
