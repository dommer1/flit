//! Experimental on-device summaries: which models the user may pick, where
//! their files live, and (later) the engine that runs them.
//!
//! PRIVACY: nothing here ever sends message content anywhere. The only
//! network use in this module is fetching a model file the user explicitly
//! chose and clicked to download — see CLAUDE.md's hard rules.

pub mod catalog;
pub mod store;
