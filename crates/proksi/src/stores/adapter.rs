// Adapter module for managing different types of store adapters.
// These are used to abstract where we store certificates, routing information etc.
// For certificates it might be a file system, a database, or a cloud storage service.
// The system will try to sync the information to ensure that a given server is not
// constantly requesting the information and adding latency to requests.
// pub trait StoreAdapter {
//     fn new() -> Self;
//     fn get(&self, key: &str) -> Option<String>;
//     fn set(&self, key: &str, value: &str) -> Result<(), String>;
//     fn delete(&self, key: &str) -> Result<(), String>;
// }

// pub struct RedisStore {
//     // Implementation details
// }

// pub struct MemoryStore {}

// impl StoreAdapter for MemoryStore {
//     fn new() -> Self {
//         MemoryStore::new()
//     }

//     fn get(&self, key: &str) -> Option<String> {
//         self.get(key)
//     }

//     fn set(&self, key: &str, value: &str) -> Result<(), String> {
//         self.set(key, value)
//     }

//     fn delete(&self, key: &str) -> Result<(), String> {
//         self.delete(key)
//     }
// }
