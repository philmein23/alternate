//! The small explosion when an alternative is deleted. Each slot in the page becomes one effect layer.

use rune::compose::{Compositor, Frame};
use rune::render::{EffectPhase, EffectSite, KIND_EFFECT};
use rune::{El, col};
use rune_fx::*;

use crate::State;

const FLASH: u32 = 1;
const SPARKS: u32 = 2;
const SMOKE: u32 = 3;

/// How long the explosion runs.
pub const LIFE: f32 = 0.55;

/// A deleted word's box, in the field's own points, and when it went up.
#[derive(Clone, Debug, PartialEq)]
pub struct Burst {
    pub rect: [f32; 4],
    pub started: Option<f64>,
}

/// How hot the flash is, `age` seconds in.
pub fn heat(age: f32) -> f32 {
    let rise = (age / 0.04).clamp(0.0, 1.0);
    rise * (-(age - 0.04).max(0.0) / 0.13).exp()
}

/// How much smoke is left, `age` seconds in.
pub fn smoke(age: f32) -> f32 {
    let rise = (age / 0.12).clamp(0.0, 1.0);
    rise * (1.0 - (age / LIFE).clamp(0.0, 1.0)).powi(2)
}

/// A small capsule at the word's centre, so the glow comes from a point and not from a box.
fn core(rect: [f32; 4]) -> [f32; 4] {
    let w = (rect[2] * 0.4).max(6.0);
    [rect[0] + (rect[2] - w) / 2.0, rect[1] + rect[3] / 2.0 - 3.0, w, 6.0]
}

fn slot(mark: u32, rect: [f32; 4], t0: f32) -> El {
    col().w(rect[2]).h(rect[3]).pin(Some(rect[0]), Some(rect[1]), None, None).effect(mark).effect_data([t0, 0.0, 0.0, 0.0]).radius(3.0).effect_bleed(120.0)
}

/// The effect slots for every running burst, placed over the field.
pub fn slots(s: &State) -> Vec<El> {
    s.bursts
        .iter()
        .filter_map(|b| b.started.map(|t| (b.rect, t as f32)))
        .flat_map(|(rect, t)| [slot(SMOKE, core(rect), t), slot(FLASH, core(rect), t), slot(SPARKS, rect, t)])
        .collect()
}

/// Starts new bursts and clears finished ones. True while any is running.
pub fn poll(s: &mut State, now: f64, tx: &mut rune::Tx) -> bool {
    if s.bursts.is_empty() {
        return false;
    }
    for b in &mut s.bursts {
        b.started.get_or_insert(now);
    }
    s.bursts.retain(|b| b.started.is_some_and(|t| (now - t) < LIFE as f64));
    tx.wake_after(std::time::Duration::from_millis(16));
    true
}

fn layer(kind: u32, d: [f32; 4], shape: Shape, t: f32) -> Option<FxLayer> {
    let age = (t - d[0]).max(0.0);
    let fx = match kind {
        FLASH => Fx::glow(GlowParams { color: rgb(0xff7a1a), core: rgb(0xfff3b0), third: rgb(0xffc247), radius: 40.0, intensity: heat(age) * 2.2, heat: heat(age), occlusion: 0.0, inner: 1.0, ..Default::default() }),
        SMOKE => Fx::glow(GlowParams { color: rgb(0x6b625a), core: rgb(0x8a8078), third: rgb(0x4c4640), radius: 48.0, intensity: smoke(age) * 0.5, occlusion: 0.0, inner: 1.0, ..Default::default() }),
        SPARKS => Fx::sparkles(SparkleParams { burst: Some(d[0]), count: 34, size: 3.2, color: rgb(0xffb347), margin: 54.0, life: LIFE, intensity: 1.6 }),
        _ => return None,
    };
    Some(fx.on(shape).at(t))
}

#[derive(Default)]
pub struct Gfx {
    fx: Option<FxRenderer>,
    moving: bool,
}

impl<S: 'static> Compositor<S> for Gfx {
    fn animating(&self) -> bool {
        self.moving
    }

    fn render(&mut self, f: &mut Frame<'_, S>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let t = f.rt.t as f32;
        let marks = &f.rt.marks;
        let mut at: Vec<u32> = Vec::new();
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut placements: Vec<FxSlot> = Vec::new();
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT {
                continue;
            }
            let kind = marks.get(n).copied().unwrap_or(0) & 0xff;
            let k = i.params[3];
            let rect = i.rect.map(|v| v / k);
            let r = i.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
            let shape = Shape::rounded(rect[0], rect[1], rect[2], rect[3], r);
            let Some(l) = layer(kind, i.uv, shape, t) else { continue };
            at.push(n as u32);
            layers.push(l);
            placements.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
        }
        self.moving = !layers.is_empty();
        let fx = self.fx.get_or_insert_with(|| FxRenderer::new(device, f.format, FxOptions { dither: true }));
        fx.set_srgb_blend(f.srgb_blend());
        fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
        fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: 1.0, layers: &layers, backdrop: None, sources: &[] }, &placements);
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, s: &EffectSite<'_>| {
            let Some(k) = at.iter().position(|n| *n == s.index) else { return };
            match phase {
                EffectPhase::Backdrop(enc) => {
                    if let Some(bd) = s.backdrop {
                        fx.blur_backdrop(device, queue, enc, bd, k);
                    }
                }
                EffectPhase::Draw(pass) => fx.draw_layer_in(pass, s.format, s.origin, k),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flash_is_hot_early_and_nearly_out_by_the_end() {
        assert!(heat(0.04) > 0.95);
        assert!(heat(LIFE) < 0.06);
    }

    #[test]
    fn smoke_builds_after_the_flash_and_is_gone_at_the_end() {
        assert!(smoke(0.0) < 0.01);
        assert!(smoke(0.14) > 0.4);
        assert!(smoke(LIFE) < 0.001);
    }
}
