//! INI parser plugin — full-parse mode on tree-sitter-ini (issue #48). INI is a KEYED
//! config format: review identity lives in `[section]` headers and setting keys, so
//! sections are labeled by their header text and settings by their key — the same
//! identity model the json/yaml/toml keyed family uses.

use intentumdiff_plugin_sdk::{
    cst::CstNode,
    ts_convert::{convert_semantic, node_to_cst},
    tree::SemanticNodeBuilder,
};

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "parser-plugin",
});

use crate::exports::intentumdiff::plugin::parser::ExamplePair;
use crate::exports::intentumdiff::plugin::parser::Guest;
use crate::exports::intentumdiff::plugin::parser::LanguageInfoRecord;
use crate::exports::intentumdiff::plugin::parser::ParserMode;

const LANGUAGE_ID: &str = "ini";
const PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

const DEFAULT_OLD: &str = "[server]\nhost = 0.0.0.0\nport = 8080\n";
const DEFAULT_NEW: &str = "[server]\nhost = 0.0.0.0\nport = 9090\nworkers = 4\n";

// Grammar node types that carry review meaning. Brackets, equals signs and comments are
// dropped (not listed, no semantic children).
const SEMANTIC_TYPES: &[&str] = &[
    "document", "section", "section_name", "setting", "setting_value",
];

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn language_info_for(ids: Vec<String>) -> Vec<LanguageInfoRecord> {
    let metadata = intentumdiff_plugin_sdk::metadata::parse_plugin_metadata(PLUGIN_METADATA);
    ids.into_iter()
        .map(|language_id| {
            let info = metadata.language_or_default(&language_id);
            LanguageInfoRecord {
                language_id: info.language_id,
                language_name: info.language_name,
                language_short_name: info.language_short_name,
                monaco_language: info.monaco_language,
                default_filename: info.default_filename,
                language_file_extensions: info.language_file_extensions,
                author: metadata.author().to_string(),
                plugin_version: metadata.plugin_version().to_string(),
                last_updated: metadata.last_updated().to_string(),
            }
        })
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(path)
}

const INI_EXTENSIONS: &[&str] = &[".ini", ".cfg", ".conf", ".editorconfig", ".gitconfig"];
const INI_FILENAMES: &[&str] = &[".editorconfig", ".gitconfig", ".gitmodules", "setup.cfg"];

fn detect_language_impl(filename: &str, _content: &str) -> String {
    let name = basename(filename).to_lowercase();
    if INI_FILENAMES.iter().any(|f| name == *f)
        || INI_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
    {
        LANGUAGE_ID.to_string()
    } else {
        String::new()
    }
}

/// The first descendant of `key_type` (a section header name or a setting key) — the
/// grammar nests settings INSIDE their section, so a section's key search must target
/// section_name specifically or it would find the first setting's key instead.
fn key_text(node: &CstNode, key_type: &str) -> Option<String> {
    fn leaf_text(node: &CstNode) -> Option<String> {
        if node.is_leaf() {
            let text = node.text_or_empty().trim();
            if !text.is_empty() {
                return Some(text.chars().take(120).collect());
            }
            return None;
        }
        node.children.iter().find_map(leaf_text)
    }
    fn find_key(node: &CstNode, key_type: &str) -> Option<String> {
        if node.node_type == key_type {
            // The name node's text lives in a `text` LEAF child (CstNode only carries
            // text on leaves — the markdown inline lesson).
            if let Some(text) = leaf_text(node) {
                return Some(text);
            }
        }
        for child in &node.children {
            if let Some(text) = find_key(child, key_type) {
                return Some(text);
            }
        }
        None
    }
    find_key(node, key_type)
}

fn label_for(node: &CstNode) -> String {
    if node.is_leaf() {
        return node.text_or_empty().trim().chars().take(120).collect();
    }
    match node.node_type.as_str() {
        "section" => key_text(node, "section_name").unwrap_or_else(|| node.node_type.clone()),
        "setting" => key_text(node, "setting_name").unwrap_or_else(|| node.node_type.clone()),
        _ => node.node_type.clone(),
    }
}

fn parse_source(source: &str) -> Result<CstNode, String> {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_ini::LANGUAGE.into();
    parser
        .set_language(&lang)
        .map_err(|_| "Failed to load INI grammar".to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse INI".to_string())?;
    Ok(node_to_cst(tree.root_node(), source.as_bytes()))
}

fn process_impl(source: &str) -> String {
    let cst = match parse_source(source) {
        Ok(cst) => cst,
        Err(err) => return format!(r#"{{"error":"{}"}}"#, err),
    };
    let mut memo = std::collections::HashMap::new();
    let node = convert_semantic(&cst, "0", &mut memo, &is_semantic, &label_for).unwrap_or_else(|| {
        SemanticNodeBuilder::new("0", "document", LANGUAGE_ID, 0, 0, 0, 0, "0").build()
    });
    match serde_json::to_string(&node) {
        Ok(serialized) => serialized,
        Err(err) => format!(r#"{{"error":"Serialisation error: {}"}}"#, err),
    }
}

struct IniParser;

impl Guest for IniParser {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }

    fn grammar_id() -> String {
        LANGUAGE_ID.to_string()
    }

    fn detect_language(filename: String, content: String) -> String {
        detect_language_impl(&filename, &content)
    }

    fn preprocess_source(source: String) -> String {
        source
    }

    fn example(_language: String) -> ExamplePair {
        ExamplePair {
            old: DEFAULT_OLD.to_string(),
            new: DEFAULT_NEW.to_string(),
        }
    }

    fn process(input: String, _language: String, _filename: String) -> String {
        process_impl(&input)
    }

    fn trivia_node_types() -> Vec<String> {
        vec![]
    }

    fn language_ids() -> Vec<String> {
        vec![LANGUAGE_ID.to_string()]
    }

    fn language_info() -> Vec<LanguageInfoRecord> {
        language_info_for(Self::language_ids())
    }

    fn priority() -> i32 {
        5
    }
}

export!(IniParser);

#[cfg(test)]
mod tests {
    use super::*;
    use intentumdiff_plugin_sdk::tree::SemanticNode;

    fn labels_by_type(node: &SemanticNode, node_type: &str, out: &mut Vec<String>) {
        if node.node_type == node_type {
            out.push(node.label.clone());
        }
        for child in &node.children {
            labels_by_type(child, node_type, out);
        }
    }

    #[test]
    fn parser_mode_is_full_parse() {
        assert_eq!(IniParser::get_parser_mode(), ParserMode::FullParse);
    }

    #[test]
    fn detects_ini_extensions_and_filenames() {
        assert_eq!(detect_language_impl("settings.ini", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl(".editorconfig", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("app.conf", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("main.rs", ""), "");
    }

    #[test]
    fn sections_and_settings_are_labeled_by_their_keys() {
        let parsed = process_impl(DEFAULT_NEW);
        intentumdiff_plugin_sdk::testing::assert_valid_json(&parsed, LANGUAGE_ID);
        let root: SemanticNode = serde_json::from_str(&parsed).unwrap();
        let mut sections = Vec::new();
        labels_by_type(&root, "section", &mut sections);
        assert_eq!(sections, vec!["server".to_string()], "sections: {sections:?}");
        let mut settings = Vec::new();
        labels_by_type(&root, "setting", &mut settings);
        assert!(settings.contains(&"port".to_string()), "settings: {settings:?}");
        assert!(settings.contains(&"workers".to_string()), "settings: {settings:?}");
    }

    #[test]
    fn value_edit_changes_the_root_hash() {
        let old: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_OLD)).unwrap();
        let new: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_NEW)).unwrap();
        assert_ne!(old.structural_hash, new.structural_hash);
    }
}
