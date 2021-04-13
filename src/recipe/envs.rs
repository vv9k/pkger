use std::collections::HashMap;
use toml::value::Table as TomlTable;

#[derive(Clone, Debug)]
pub struct Env(HashMap<String, String>);

impl From<Option<TomlTable>> for Env {
    fn from(env: Option<TomlTable>) -> Self {
        let mut data = HashMap::new();

        if let Some(env) = env {
            env.into_iter().for_each(|(k, v)| {
                data.insert(k, v.to_string().trim_matches('"').to_string());
            });
        }

        Env(data)
    }
}

impl Env {
    pub fn insert<K, V>(&mut self, key: K, value: V) -> Option<String>
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.0.insert(key.into(), value.into())
    }

    #[allow(dead_code)]
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
}
