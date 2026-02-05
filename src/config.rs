use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Site configuration loaded from config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteConfig {
    pub colors: ColorConfig,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            colors: ColorConfig::default(),
        }
    }
}

/// Color configuration for light and dark modes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorConfig {
    pub light: ColorScheme,
    pub dark: ColorScheme,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            light: ColorScheme::default_light(),
            dark: ColorScheme::default_dark(),
        }
    }
}

/// Individual color scheme
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorScheme {
    /// Background color
    pub background: String,
    /// Primary text color
    pub text: String,
    /// Muted/secondary text color (used for nav menu, breadcrumbs)
    pub text_muted: String,
    /// Border color
    pub border: String,
    /// Link color
    pub link: String,
    /// Link hover color
    pub link_hover: String,
}

impl ColorScheme {
    pub fn default_light() -> Self {
        Self {
            background: "#ffffff".to_string(),
            text: "#111111".to_string(),
            text_muted: "#666666".to_string(),
            border: "#e0e0e0".to_string(),
            link: "#333333".to_string(),
            link_hover: "#000000".to_string(),
        }
    }

    pub fn default_dark() -> Self {
        Self {
            background: "#0a0a0a".to_string(),
            text: "#eeeeee".to_string(),
            text_muted: "#999999".to_string(),
            border: "#333333".to_string(),
            link: "#cccccc".to_string(),
            link_hover: "#ffffff".to_string(),
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::default_light()
    }
}

/// Load config from config.toml in the given directory
pub fn load_config(root: &Path) -> Result<SiteConfig, ConfigError> {
    let config_path = root.join("config.toml");
    if !config_path.exists() {
        return Ok(SiteConfig::default());
    }

    let content = fs::read_to_string(&config_path)?;
    let config: SiteConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Generate CSS custom properties from color config
pub fn generate_color_css(colors: &ColorConfig) -> String {
    format!(
        r#":root {{
    --color-bg: {light_bg};
    --color-text: {light_text};
    --color-text-muted: {light_text_muted};
    --color-border: {light_border};
    --color-link: {light_link};
    --color-link-hover: {light_link_hover};
}}

@media (prefers-color-scheme: dark) {{
    :root {{
        --color-bg: {dark_bg};
        --color-text: {dark_text};
        --color-text-muted: {dark_text_muted};
        --color-border: {dark_border};
        --color-link: {dark_link};
        --color-link-hover: {dark_link_hover};
    }}
}}"#,
        light_bg = colors.light.background,
        light_text = colors.light.text,
        light_text_muted = colors.light.text_muted,
        light_border = colors.light.border,
        light_link = colors.light.link,
        light_link_hover = colors.light.link_hover,
        dark_bg = colors.dark.background,
        dark_text = colors.dark.text,
        dark_text_muted = colors.dark.text_muted,
        dark_border = colors.dark.border,
        dark_link = colors.dark.link,
        dark_link_hover = colors.dark.link_hover,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_colors() {
        let config = SiteConfig::default();
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
    }

    #[test]
    fn parse_partial_config() {
        let toml = r##"
[colors.light]
background = "#fafafa"
"##;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        // Overridden value
        assert_eq!(config.colors.light.background, "#fafafa");
        // Default values preserved
        assert_eq!(config.colors.light.text, "#111111");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
    }

    #[test]
    fn generate_css_uses_config_colors() {
        let mut colors = ColorConfig::default();
        colors.light.background = "#f0f0f0".to_string();
        colors.dark.background = "#1a1a1a".to_string();

        let css = generate_color_css(&colors);
        assert!(css.contains("--color-bg: #f0f0f0"));
        assert!(css.contains("--color-bg: #1a1a1a"));
    }
}
