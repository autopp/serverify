use indexmap::IndexMap;
use serde_json::Value;

pub struct JsonTemplate {
    template: serde_json::Value,
    placeholder_paths: Vec<PlaceholderPath>,
}

enum PlaceholderPath {
    PlaceHolder(String),
    Index(usize, Box<PlaceholderPath>),
    Field(String, Box<PlaceholderPath>),
}

impl JsonTemplate {
    pub fn parse(template: serde_json::Value, placeholders: Vec<String>) -> Result<Self, String> {
        let placeholder_paths = Self::traverse(&template, &placeholders);

        Ok(Self {
            template,
            placeholder_paths,
        })
    }

    pub fn expand(&self, values: IndexMap<String, Value>) -> serde_json::Value {
        let mut expanded = self.template.clone();

        self.placeholder_paths.iter().for_each(|placeholder_path| {
            Self::expand_recursive(&mut expanded, &values, placeholder_path);
        });

        expanded
    }

    fn traverse(template: &serde_json::Value, placeholders: &[String]) -> Vec<PlaceholderPath> {
        match template {
            serde_json::Value::Object(obj) => obj
                .iter()
                .flat_map(|(key, value)| {
                    Self::traverse(value, placeholders)
                        .into_iter()
                        .map(|path| PlaceholderPath::Field(key.to_string(), Box::new(path)))
                })
                .collect(),
            serde_json::Value::Array(arr) => arr
                .iter()
                .enumerate()
                .flat_map(|(index, value)| {
                    Self::traverse(value, placeholders)
                        .into_iter()
                        .map(move |path| PlaceholderPath::Index(index, Box::new(path)))
                })
                .collect(),
            serde_json::Value::String(s) if placeholders.contains(s) => {
                vec![PlaceholderPath::PlaceHolder(s.clone())]
            }
            _ => vec![],
        }
    }

    fn expand_recursive(
        expanded: &mut serde_json::Value,
        values: &IndexMap<String, Value>,
        placeholder_path: &PlaceholderPath,
    ) {
        match placeholder_path {
            PlaceholderPath::PlaceHolder(placeholder) => {
                let value = values.get(placeholder).unwrap();
                *expanded = value.clone();
            }
            PlaceholderPath::Index(index, path) => {
                let expanded = expanded.as_array_mut().unwrap();
                let value = expanded.get_mut(*index).unwrap();
                Self::expand_recursive(value, values, path);
            }
            PlaceholderPath::Field(field, path) => {
                let expanded = expanded.as_object_mut().unwrap();
                let value = expanded.get_mut(field).unwrap();
                Self::expand_recursive(value, values, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::indexmap;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use serde_json::json;

    #[rstest]
    #[case(json!("$_value"), indexmap! { "$_value".to_string() => json!([1, 2, 3]) }, json!([1, 2, 3]))]
    #[case(json!({"a": 1, "b": "$_value"}), indexmap! { "$_value".to_string() => json!(42) }, json!({"a": 1, "b": 42}))]
    #[case(json!([{"index": "$_index", "value": 41}, {"index": 1, "value": "$_value"}]), indexmap! { "$_index".to_string() => json!(0), "$_value".to_string() => json!(42) }, json!([{"index": 0, "value": 41}, {"index": 1, "value": 42}]))]
    fn success_cases(
        #[case] template: serde_json::Value,
        #[case] value: IndexMap<String, Value>,
        #[case] expected: serde_json::Value,
    ) {
        let template =
            JsonTemplate::parse(template, vec!["$_index".to_string(), "$_value".to_string()])
                .unwrap();
        let actual = template.expand(value);
        assert_eq!(expected, actual);
    }
}
