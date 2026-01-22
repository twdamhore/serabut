use std::collections::HashMap;
use std::path::Path;

use minijinja::{Environment, Value};

use crate::error::AppError;

/// Render a Jinja2 template file with the given context
pub fn render_template(
    template_path: &Path,
    context: &HashMap<String, String>,
) -> Result<String, AppError> {
    let template_content = std::fs::read_to_string(template_path)?;

    let mut env = Environment::new();
    env.add_template("template", &template_content)?;

    let template = env.get_template("template")?;

    // Convert HashMap to minijinja Value
    let ctx: HashMap<&str, Value> = context
        .iter()
        .map(|(k, v)| (k.as_str(), Value::from(v.clone())))
        .collect();

    let rendered = template.render(ctx)?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_render_template() {
        let mut file = NamedTempFile::new().unwrap();
        // Use write_all to avoid format string interpretation of braces
        file.write_all(b"Hello {{ hostname }}! Host: {{ host }}:{{ port }}\n").unwrap();

        let mut ctx = HashMap::new();
        ctx.insert("hostname".to_string(), "testhost".to_string());
        ctx.insert("host".to_string(), "192.168.1.1".to_string());
        ctx.insert("port".to_string(), "8080".to_string());

        let result = render_template(file.path(), &ctx).unwrap();
        // MiniJinja preserves the trailing newline from the template
        assert!(result.starts_with("Hello testhost! Host: 192.168.1.1:8080"));
    }
}
