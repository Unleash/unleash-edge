use http::HeaderMap;
use serde::{
    Serialize, Serializer,
    ser::{Error as _, SerializeMap},
};

pub(crate) struct SerializableHeaders<'a>(pub(crate) &'a HeaderMap);

impl Serialize for SerializableHeaders<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;

        for (name, value) in self.0 {
            let value = value.to_str().map_err(S::Error::custom)?;

            map.serialize_entry(name.as_str(), value)?;
        }

        map.end()
    }
}
