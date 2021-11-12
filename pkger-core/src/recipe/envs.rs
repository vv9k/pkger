use serde_yaml::Mapping;
use std::collections::HashMap;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Env(HashMap<String, String>);

impl From<Option<Mapping>> for Env {
    fn from(env: Option<Mapping>) -> Self {
        let mut data = HashMap::new();

        if let Some(env) = env {
            env.into_iter()
                .filter(|(k, v)| k.is_string() && v.is_string())
                .for_each(|(k, v)| {
                    data.insert(
                        k.as_str().unwrap().to_string(),
                        v.as_str().unwrap().to_string(),
                    );
                });
        }

        Env(data)
    }
}

impl Env {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<K, V>(&mut self, key: K, value: V) -> Option<String>
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.0.insert(key.into(), value.into())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn remove<K>(&mut self, key: K) -> Option<String>
    where
        K: AsRef<str>,
    {
        self.0.remove(key.as_ref())
    }

    pub fn kv_vec(self) -> Vec<String> {
        self.0
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }

    pub fn inner(&self) -> &HashMap<String, String> {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_entries_as_vars() {
        let mut env = Env::new();
        assert!(env.is_empty());

        env.insert("key", "val");
        env.insert("second", "val2");

        let envs = env.clone().kv_vec();

        assert!(envs.contains(&"key=val".to_string()));
        assert!(envs.contains(&"second=val2".to_string()));

        env.remove("key");
        env.remove("second");
        assert!(env.is_empty());
    }
}
