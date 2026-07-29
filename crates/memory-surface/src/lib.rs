//! Versioned cognitive surfaces and memberships for Jiangcheng Memory.

/// Surface lifecycle commands.
pub mod lifecycle;
/// Surface membership and evidence types.
pub mod membership;
/// Versioned cognitive surface domain types.
pub mod model;
/// Scope-bound surface repository.
pub mod repository;

pub use lifecycle::{
    AddMember, CreateSurface, DeactivateSurface, FreezeSurface, MergeSurfaces, RemoveMember,
    SplitSurface, SurfacePartition,
};
pub use membership::{SurfaceConflict, SurfaceEvidence, SurfaceMembership, SurfacePoint};
pub use model::{CognitiveSurface, SurfaceBoundary, SurfaceVersion};
pub use repository::{SurfaceRepository, SurfaceRepositoryError};
