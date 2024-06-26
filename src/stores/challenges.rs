use dashmap::DashMap;

pub type ChallengeStore = DashMap<String, (String, String)>;
