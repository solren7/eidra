// Cross-cutting infra (LLM backend, tool adapter, workday calendar)
pub mod llm;
pub mod rig_tool;
pub mod workday;

// Layered infra by concern
pub mod memory;
pub mod messaging;
pub mod persistence;
