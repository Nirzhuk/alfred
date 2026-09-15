//! Portable workflow templates. This module is deliberately independent from
//! the runner and database so the same validation rules can be used by export,
//! preview, service fixtures, and import transactions.
pub mod contract;

pub use contract::{
    apply_parameter_values, canonical_bytes, digest, export_graph, export_workflow,
    export_workflow_with_starts, remap_graph_ids, validate_template, ImportGraph, Parameter,
    ParameterKind, ParameterTarget, Requirement, RequirementKind, StartKind, StartRecipe,
    TargetKind, TemplateEdge, TemplateEnvelope, TemplateError, TemplateGraph, TemplateNode,
};
