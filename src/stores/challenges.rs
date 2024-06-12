use std::sync::Arc;

use dashmap::DashMap;

pub type ChallengeStore = Arc<DashMap<String, (String, String)>>;
