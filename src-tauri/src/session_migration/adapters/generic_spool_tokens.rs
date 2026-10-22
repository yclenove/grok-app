use serde::de::Error as _;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

pub(super) struct NumberSeed;

impl<'de> DeserializeSeed<'de> for NumberSeed {
    type Value = Option<u64>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(NumberVisitor)
    }
}

struct NumberVisitor;

impl<'de> Visitor<'de> for NumberVisitor {
    type Value = Option<u64>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a numeric token count")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Some(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(u64::try_from(value).ok())
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok((value.is_finite() && value >= 0.0).then_some(value as u64))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

#[derive(Default)]
pub(super) struct UsageTotals {
    total_tokens: Option<Option<u64>>,
    total_tokens_camel: Option<Option<u64>>,
}

impl UsageTotals {
    pub(super) fn value(&self) -> Option<u64> {
        match self.total_tokens {
            Some(value) => value,
            None => self.total_tokens_camel.flatten(),
        }
    }
}

pub(super) struct UsageSeed;

impl<'de> DeserializeSeed<'de> for UsageSeed {
    type Value = UsageTotals;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UsageVisitor)
    }
}

struct UsageVisitor;

impl<'de> Visitor<'de> for UsageVisitor {
    type Value = UsageTotals;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a usage object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut totals = UsageTotals::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "total_tokens" => {
                    if totals.total_tokens.is_some() {
                        return Err(A::Error::duplicate_field("total_tokens"));
                    }
                    totals.total_tokens = Some(map.next_value_seed(NumberSeed)?);
                }
                "totalTokens" => {
                    if totals.total_tokens_camel.is_some() {
                        return Err(A::Error::duplicate_field("totalTokens"));
                    }
                    totals.total_tokens_camel = Some(map.next_value_seed(NumberSeed)?);
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(totals)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(UsageTotals::default())
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UsageTotals::default())
    }
}
