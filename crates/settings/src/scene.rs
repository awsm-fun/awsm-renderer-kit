//! The renderer's mode as a scene file's anti-aliasing block.

use awsm_renderer::anti_alias::AntiAliasing;
use awsm_renderer::taa::{TaaBloomSource, TaaDisocclusion, TaaDofOrder};
use awsm_renderer_scene::{
    AntiAliasingConfig, TaaBloomSourceSetting, TaaDisocclusionSetting, TaaDofOrderSetting,
    TaaSettings,
};

/// `aa` as `scene.post_process.anti_aliasing`. `load_scene_for_player`
/// applies a scene's authored anti-aliasing and replaces the renderer's, so
/// an app that lets the user choose copies its choice into the scene before
/// loading: the loader then changes nothing, and nothing compiles twice.
///
/// TAA keeps its render scale, sharpening, disocclusion level, bloom source
/// and DoF order; its tuning becomes the shipped default (the kit's modes
/// never change it).
pub fn scene_anti_aliasing(aa: &AntiAliasing) -> AntiAliasingConfig {
    AntiAliasingConfig {
        msaa: aa.msaa_sample_count.is_some(),
        smaa: aa.smaa,
        taa: aa.taa.as_ref().map(|taa| TaaSettings {
            render_scale: taa.render_scale,
            sharpen: taa.sharpen,
            disocclusion: match taa.disocclusion {
                TaaDisocclusion::Depth => TaaDisocclusionSetting::Depth,
                TaaDisocclusion::DepthAndMotion => TaaDisocclusionSetting::DepthAndMotion,
                TaaDisocclusion::DepthMotionAndId => TaaDisocclusionSetting::DepthMotionAndId,
            },
            bloom_source: match taa.bloom_source {
                TaaBloomSource::CurrentFrame => TaaBloomSourceSetting::CurrentFrame,
                TaaBloomSource::Resolved => TaaBloomSourceSetting::Resolved,
            },
            dof_order: match taa.dof_order {
                TaaDofOrder::BeforeResolve => TaaDofOrderSetting::BeforeResolve,
                TaaDofOrder::AfterResolve => TaaDofOrderSetting::AfterResolve,
            },
            ..TaaSettings::default()
        }),
    }
}
