use dashmap::DashMap;

pub type PathCacheStorage = DashMap<String, String>;
