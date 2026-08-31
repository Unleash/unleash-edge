use http::HeaderMap;
use serde::{
    Serialize, Serializer,
    ser::SerializeMap,
};

pub(crate) struct SerializableHeaders<'a>(pub(crate) &'a HeaderMap);

impl Serialize for SerializableHeaders<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;

        for (name, value) in self.0 {
            let Ok(value) = value.to_str() else {
                continue;
            };

            map.serialize_entry(name.as_str(), value)?;
        }

        map.end()
    }
}
