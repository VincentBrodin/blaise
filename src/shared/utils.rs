use std::collections::HashMap;

use crate::repository::Slice;

pub struct StrSliceMap {
    buffer: Option<String>,
    buffer_map: HashMap<String, Slice>,
}

impl StrSliceMap {
    pub fn new() -> Self {
        Self {
            buffer: Some(String::with_capacity(1024)),
            buffer_map: HashMap::with_capacity(1024),
        }
    }

    pub fn get_slice(&mut self, key: String) -> Slice {
        let buf = self.buffer.get_or_insert(String::with_capacity(1024));

        if let Some(slice) = self.buffer_map.get(key.as_str()).copied() {
            slice
        } else {
            let slice = Slice {
                start_idx: buf.len() as u32,
                count: key.len() as u32,
            };
            buf.push_str(key.as_str());
            self.buffer_map.insert(key, slice);
            slice
        }
    }

    pub fn take(&mut self) -> String {
        self.buffer.take().unwrap_or_default()
    }
}
