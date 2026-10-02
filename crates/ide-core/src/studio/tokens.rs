//! Source token names are preserved; presentation must also recognize their values.
use super::*;

pub fn resolved_token_value<'a>(
    name: &str,
    tokens: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    let mut value = tokens.get(name)?.trim();
    // Bound alias traversal so a cyclic draft remains editable.
    for _ in 0..=tokens.len() {
        let Some(reference) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) else {
            return Some(value);
        };
        let (reference, fallback) = reference
            .split_once(',')
            .map_or((reference, None), |(name, fallback)| {
                (name, Some(fallback.trim()))
            });
        let name = reference.trim().strip_prefix("--")?;
        value = tokens.get(name).map(|v| v.trim()).or(fallback)?;
    }
    None
}

fn is_color(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if [
        "rgb(",
        "rgba(",
        "hsl(",
        "hsla(",
        "hwb(",
        "lab(",
        "lch(",
        "oklab(",
        "oklch(",
        "color(",
        "color-mix(",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
        && value.ends_with(')')
    {
        return true;
    }
    // CSS named colors, including names that are also commonly used for source tokens.
    "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen transparent currentcolor"
        .split_whitespace().any(|name| name == value)
}

pub fn system_token_group(name: &str, tokens: &BTreeMap<String, String>) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("shadow-") || lower.ends_with("-shadow") {
        "Shadows"
    } else if lower.starts_with("color-")
        || resolved_token_value(name, tokens).is_some_and(is_color)
    {
        "Colors"
    } else if lower.contains("font")
        || lower.starts_with("line-height")
        || lower.starts_with("letter-spacing")
        || [
            "body-size",
            "label-size",
            "page-title-size",
            "section-title-size",
        ]
        .contains(&lower.as_str())
    {
        "Typography"
    } else if lower.contains("radius") {
        "Corners"
    } else if lower.starts_with("space-")
        || lower.starts_with("spacing-")
        || lower.contains("gutter")
        || lower.ends_with("-gap")
        || lower.ends_with("-height")
        || lower.ends_with("-width")
    {
        "Spacing"
    } else {
        "Other"
    }
}

pub(super) fn missing_token_references<'a>(
    tokens: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    values: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<String> {
    static REFERENCE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let reference = REFERENCE.get_or_init(|| {
        regex::Regex::new(r"var\(\s*--([a-zA-Z0-9-]+)\s*([,)])")
            .expect("static CSS variable expression")
    });
    values
        .into_iter()
        .flat_map(|value| {
            reference
                .captures_iter(value)
                .filter(|capture| {
                    &capture[2] == ")"
                        && !tokens.contains_key(&capture[1])
                        && !locals.contains(&capture[1])
                })
                .map(|capture| capture[1].to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn missing_system_tokens(system: &StudioDesignSystem) -> BTreeSet<String> {
    missing_token_references(
        &system.tokens,
        &BTreeSet::new(),
        system
            .tokens
            .values()
            .chain(system.recipes.values().flat_map(|recipe| recipe.values()))
            .map(String::as_str),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_and_aliases_have_the_same_groups_as_semantic_tokens() {
        let tokens = BTreeMap::from([
            ("cyan".into(), "#00dce5".into()),
            ("paper".into(), "#fff".into()),
            ("ink".into(), "rgb(16 18 17)".into()),
            ("accent".into(), "var( --cyan )".into()),
            ("fallback".into(), "var(--missing, coral)".into()),
            ("body-font".into(), "Manrope, Arial, sans-serif".into()),
            ("body-size".into(), "13px".into()),
            ("page-gutter".into(), "16px".into()),
            ("card-radius".into(), "8px".into()),
            ("card-shadow".into(), "0 2px 4px #0003".into()),
        ]);
        for name in ["cyan", "paper", "ink", "accent", "fallback"] {
            assert_eq!(system_token_group(name, &tokens), "Colors", "{name}");
        }
        assert_eq!(resolved_token_value("accent", &tokens), Some("#00dce5"));
        assert_eq!(system_token_group("body-font", &tokens), "Typography");
        assert_eq!(system_token_group("body-size", &tokens), "Typography");
        assert_eq!(system_token_group("page-gutter", &tokens), "Spacing");
        assert_eq!(system_token_group("card-radius", &tokens), "Corners");
        assert_eq!(system_token_group("card-shadow", &tokens), "Shadows");
    }

    #[test]
    fn cyclic_aliases_do_not_hang_the_library() {
        let tokens = BTreeMap::from([
            ("first".into(), "var(--second)".into()),
            ("second".into(), "var(--first)".into()),
        ]);
        assert_eq!(resolved_token_value("first", &tokens), None);
        assert_eq!(system_token_group("first", &tokens), "Other");
    }

    #[test]
    fn unresolved_recipe_and_token_references_are_reported_but_fallbacks_are_allowed() {
        let system = StudioDesignSystem {
            tokens: BTreeMap::from([
                ("ink".into(), "#101211".into()),
                ("alias".into(), "var( --absent )".into()),
            ]),
            recipes: BTreeMap::from([(
                "button".into(),
                BTreeMap::from([
                    ("color".into(), "var(--ink)".into()),
                    ("background".into(), "var(--new-btn-primary)".into()),
                    ("border-color".into(), "var(--optional, #ddd)".into()),
                ]),
            )]),
            ..empty_system()
        };
        assert_eq!(
            missing_system_tokens(&system),
            BTreeSet::from(["absent".into(), "new-btn-primary".into()])
        );
    }
}
