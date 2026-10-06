use super::*;
use crate::services::asset_management::{
    AssetTypeId,
    formats::texture::{TextureAsset, cpu_texture_loader},
};

fn provider() -> AssetProvider {
    let mut bytes = b"IPPT\x03\0\0\0\x01\0\0\0\x01\0\0\0".to_vec();
    bytes.extend([20, 80, 140, 128]);
    let mut provider = AssetProvider::new(
        AssetKey {
            slot: 0,
            generation: 1,
        },
        AssetSource {
            kind: AssetTypeId(2),
            uri: "fixture:immutable".into(),
            variant: 0,
        },
        Rc::new(|| Box::new(TypedAssetLoader(cpu_texture_loader()))),
    );
    provider.data = Some(super::super::export::shared_cpu_data(
        TextureAsset::decode(&bytes).unwrap(),
    ));
    provider
}

#[test]
fn snapshot_shares_single_payload_and_unload_revokes_availability_without_dangling_data() {
    let mut provider = provider();
    let first = provider.cpu_export_snapshot().unwrap();
    let second = provider.cpu_export_snapshot().unwrap();
    assert!(Rc::ptr_eq(&first.data, &second.data));
    assert!(!first.available.is_cancelled());
    provider.unload(&mut Vec::new());
    assert!(first.available.is_cancelled());
    assert!(provider.cpu_export_snapshot().is_none());
    assert_eq!(
        first.data.downcast_ref::<TextureAsset>().unwrap().pixels(),
        &[20, 80, 140, 128]
    );
}

#[test]
fn graphics_loss_and_provider_drop_have_independent_cpu_lifetimes() {
    let mut provider = provider();
    let cpu = provider.cpu_export_snapshot().unwrap();
    let gpu = provider.gpu_export_availability();
    provider.invalidate_graphics(&mut Vec::new());
    assert!(gpu.is_cancelled());
    assert!(!cpu.available.is_cancelled());
    assert!(!provider.gpu_export_availability().is_cancelled());
    drop(provider);
    assert!(cpu.available.is_cancelled());
    assert_eq!(cpu.data.downcast_ref::<TextureAsset>().unwrap().width(), 1);
}
