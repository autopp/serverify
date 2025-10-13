use indexmap::IndexMap;
use minijinja::Environment;
use regex::Regex;
use serde_json::Value;

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct JsonTemplate {
    template: serde_json::Value,
    placeholder_paths: Vec<PlaceholderPath>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
enum PlaceholderPath {
    ValuePlaceholder(String),
    TextPlaceholder(String),
    Index(usize, Box<PlaceholderPath>),
    Field(String, Box<PlaceholderPath>),
}

impl JsonTemplate {
    pub fn parse(
        template: serde_json::Value,
        value_placeholders: Vec<String>,
        text_placeholder: String,
    ) -> Result<Self, String> {
        // Validate arguments
        let mut validated_names: Vec<&str> = vec![];
        for name in value_placeholders.iter() {
            Self::validate_placeholder_name(name, &validated_names)?;
            validated_names.push(name);
        }
        Self::validate_placeholder_name(&text_placeholder, &validated_names)?;

        // Parse template
        let placeholder_paths = Self::traverse(
            &template,
            &value_placeholders,
            &format!("${}", text_placeholder),
        );

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

    fn validate_placeholder_name(new_name: &str, existing_names: &[&str]) -> Result<(), String> {
        if new_name.is_empty() {
            return Err("placeholder name cannot be empty".to_string());
        }

        if !Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$")
            .unwrap()
            .is_match(new_name)
        {
            return Err(format!("invalid placeholder name: `{}`", new_name));
        }

        if existing_names.contains(&new_name) {
            return Err(format!("duplicated placeholder name: `{}`", new_name));
        }
        Ok(())
    }

    fn traverse(
        template: &serde_json::Value,
        placeholders: &[String],
        text_placeholder: &str,
    ) -> Vec<PlaceholderPath> {
        match template {
            serde_json::Value::Object(obj) => {
                if let Some(serde_json::Value::String(text)) = obj.get(text_placeholder) {
                    vec![PlaceholderPath::TextPlaceholder(text.clone())]
                } else {
                    obj.iter()
                        .flat_map(|(key, value)| {
                            Self::traverse(value, placeholders, text_placeholder)
                                .into_iter()
                                .map(|path| PlaceholderPath::Field(key.to_string(), Box::new(path)))
                        })
                        .collect()
                }
            }
            serde_json::Value::Array(arr) => arr
                .iter()
                .enumerate()
                .flat_map(|(index, value)| {
                    Self::traverse(value, placeholders, text_placeholder)
                        .into_iter()
                        .map(move |path| PlaceholderPath::Index(index, Box::new(path)))
                })
                .collect(),
            serde_json::Value::String(s)
                if s.starts_with("$") && placeholders.contains(&s[1..].to_string()) =>
            {
                vec![PlaceholderPath::ValuePlaceholder(s[1..].to_string())]
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
            PlaceholderPath::ValuePlaceholder(value) => {
                let value = values.get(value).unwrap();
                *expanded = value.clone();
            }
            PlaceholderPath::TextPlaceholder(text) => {
                let mut env = Environment::new();
                env.add_template("template", text).unwrap();
                let tmpl = env.get_template("template").unwrap();
                let rendered = tmpl.render(values).unwrap();

                *expanded = Value::String(rendered);
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
    #[case(json!("$_value"), indexmap! { "_value".to_string() => json!([1, 2, 3]) }, json!([1, 2, 3]))]
    #[case(json!({"a": 1, "b": "$_value"}), indexmap! { "_value".to_string() => json!(42) }, json!({"a": 1, "b": 42}))]
    #[case(json!([{"index": "$_index", "value": 41}, {"index": 1, "value": "$_value"}]), indexmap! { "_index".to_string() => json!(0), "_value".to_string() => json!(42) }, json!([{"index": 0, "value": 41}, {"index": 1, "value": 42}]))]
    #[case(json!({"text": { "$_text": "index: {{ _index }}, value: {{ _value }}" }}), indexmap! { "_index".to_string() => json!(0), "_value".to_string() => json!(42) }, json!({"text": "index: 0, value: 42"}))]
    fn expand(
        #[case] template: serde_json::Value,
        #[case] values: IndexMap<String, Value>,
        #[case] expected: serde_json::Value,
    ) {
        let template = JsonTemplate::parse(
            template,
            vec!["_index".to_string(), "_value".to_string()],
            "_text".to_string(),
        )
        .unwrap();
        let actual = template.expand(values);
        assert_eq!(expected, actual);
    }

    mod parse {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        #[case(vec![""], "_text", "placeholder name cannot be empty")]
        #[case(vec!["_value"], "", "placeholder name cannot be empty")]
        #[case(vec!["$abc"], "_text", "invalid placeholder name: `$abc`")]
        #[case(vec!["_value"], "0x", "invalid placeholder name: `0x`")]
        #[case(vec!["abc", "def", "abc"], "_text", "duplicated placeholder name: `abc`")]
        #[case(vec!["abc", "def"], "abc", "duplicated placeholder name: `abc`")]
        fn failure_cases(
            #[case] value_placeholders: Vec<&'static str>,
            #[case] text_placeholder: &'static str,
            #[case] expected_error_message: &'static str,
        ) {
            let template = json!({
                "a": 1,
                "b": "$_value",
                "c": { "$_text": "value: {{ _value }}" },
            });

            let result = JsonTemplate::parse(
                template,
                value_placeholders.iter().map(|s| s.to_string()).collect(),
                text_placeholder.to_string(),
            );
            assert_eq!(result, Err(expected_error_message.to_string()));
        }
    }
}
