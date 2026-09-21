mod culling_views;
mod draw_probe;
mod draw_sweep;
mod fixture;
mod measurement;
mod pose_probes;

pub(super) use measurement::run;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Renderer = ipp_render_gl::RenderService<ipp_render_gl::GlesRenderDevice>;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
