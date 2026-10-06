//! Camera selection, navigation, viewport projection and view publication.

mod camera_state;
pub use camera_state::{CameraStateChange, CameraStatePatch};

mod component;
pub use component::Camera;

mod navigation;
pub use navigation::CameraMotion;

mod projection;
pub use projection::{PreparedCamera, prepare, prepare_affine, prepare_affine_for_extent};

mod publication;
pub use publication::{CameraPublication, CameraViewRay};

mod system;
pub use system::{CameraSystem, CameraSystemFactory};

mod system_state;
pub use system_state::CameraSystemState;

mod update;
pub(in crate::world) use update::CameraReadAccess;

mod view;
pub use view::{CameraNavigationCommand, CameraProjectionView, CameraViewMotion};
