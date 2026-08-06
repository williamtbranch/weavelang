// src/services/illustration/mod.rs
//
// Illustration prompt generation with deterministic character consistency.
// See documentation/Illustration_Consistency_Plan.md.
//
// Stage map:
//   S0 preflight     orchestrator::run
//   S2 bible         extract  -> bible (frozen, lock-aware merge)
//   S4 scene plan    scene_plan (structured JSON, concurrent)
//   S5 fold          render::resolve_state
//   S6 render        render    (deterministic — no LLM)
//   S7 lint          lint      (deterministic — no LLM)
//   S9 key art       thumbnail (one call, then deterministic render)
//   S8 write         output
//
// Only S2, S4 and S9 call the LLM. Everything that determines what a character
// *looks like* is deterministic, which is what prevents drift between images.

pub mod bible;
pub mod extract;
pub mod lint;
pub mod llm;
pub mod orchestrator;
pub mod output;
pub mod render;
pub mod scene_plan;
pub mod segment;
pub mod thumbnail;
pub mod types;
