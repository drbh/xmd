//! The default inlay pipeline uses the workspace's feature modules.
use crate::inlays::InlayFeature;

pub const BUILTINS: &[&dyn InlayFeature] = &[&super::module_inlays::ModuleInlays];
