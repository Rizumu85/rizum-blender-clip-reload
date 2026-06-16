use crate::stack_plan::StrictRasterStackOptions;

pub(super) fn tile_silo_options() -> StrictRasterStackOptions {
    StrictRasterStackOptions {
        allow_alpha_compositing: true,
        allow_paper: true,
        allow_layer_opacity: true,
        allow_masks: true,
        allow_clipping_runs: true,
        allow_container_isolation: true,
        allow_through_groups: true,
        allow_add_blend: true,
        allow_add_glow_blend: true,
        allow_color_burn_blend: true,
        allow_color_dodge_blend: true,
        allow_extended_blends: true,
        allow_glow_dodge_blend: true,
        allow_hard_mix_blend: true,
        allow_hsl_blends: true,
        allow_simple_blends: true,
        allow_soft_light_blend: true,
        allow_lut_filters: true,
        allow_vivid_light_blend: true,
        allow_w3c_blends: true,
        allow_initial_terminal_container_elision: true,
    }
}
