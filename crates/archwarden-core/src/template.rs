//! The `{{pascal(name)}}` mini-template used by `naming` rules.
//!
//! A `naming` rule captures groups out of the filename with a regex, then
//! builds the required export name from them. `docs/CONFIG.md` calls this a
//! "small templating helper" and that is exactly the ambition: one placeholder
//! form, seven case transforms, no conditionals, no loops.

use std::fmt;

/// The case transforms available inside a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaseTransform {
    /// `create-client` becomes `CreateClient`.
    Pascal,
    /// `create-client` becomes `createClient`.
    Camel,
    /// `CreateClient` becomes `create-client`.
    Kebab,
    /// `CreateClient` becomes `create_client`.
    Snake,
    /// Uppercases every character, leaving separators alone.
    Upper,
    /// Lowercases every character, leaving separators alone.
    Lower,
    /// Passes the capture through untouched.
    Raw,
}

impl CaseTransform {
    /// Every transform, in the order `docs/RULES.md` lists them. Used to build
    /// error messages, so the list a user sees is never out of date.
    pub const ALL: [Self; 7] = [
        Self::Pascal,
        Self::Camel,
        Self::Kebab,
        Self::Snake,
        Self::Upper,
        Self::Lower,
        Self::Raw,
    ];

    /// Parses a transform by the name used in a config template.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The name this transform is written as in a config template.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pascal => "pascal",
            Self::Camel => "camel",
            Self::Kebab => "kebab",
            Self::Snake => "snake",
            Self::Upper => "upper",
            Self::Lower => "lower",
            Self::Raw => "raw",
        }
    }

    /// Applies the transform.
    #[must_use]
    pub fn apply(self, input: &str) -> String {
        use heck::{ToKebabCase, ToLowerCamelCase, ToPascalCase, ToSnakeCase};

        match self {
            Self::Pascal => input.to_pascal_case(),
            Self::Camel => input.to_lower_camel_case(),
            Self::Kebab => input.to_kebab_case(),
            Self::Snake => input.to_snake_case(),
            // `upper` and `lower` are case changes, not word-boundary
            // reshaping: separators are left exactly where they were. A caller
            // who wants `CREATE_CLIENT` composes `snake` with `upper` at the
            // config level rather than getting it by accident here.
            Self::Upper => input.to_uppercase(),
            Self::Lower => input.to_lowercase(),
            Self::Raw => input.to_owned(),
        }
    }
}

impl fmt::Display for CaseTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a template could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TemplateError {
    /// A `{{` was opened and never closed.
    #[error("unclosed `{{{{` in template `{template}`")]
    Unclosed {
        /// The template as written.
        template: String,
    },
    /// A placeholder was not of the form `transform(group)`.
    #[error("placeholder `{{{{{placeholder}}}}}` must look like `pascal(name)`")]
    Malformed {
        /// The placeholder body, without the braces.
        placeholder: String,
    },
    /// The transform name is not one we ship.
    #[error("unknown transform `{name}` (available: {available})")]
    UnknownTransform {
        /// The name as written.
        name: String,
        /// Comma-separated list of valid names.
        available: String,
    },
    /// The template referenced a capture group the regex does not define.
    #[error("template references capture group `{group}`, which `file_pattern` does not define")]
    UnknownGroup {
        /// The group name as written.
        group: String,
    },
}

/// Renders a template, resolving each `{{transform(group)}}` through `lookup`.
///
/// `lookup` returns `None` for a group the regex does not define, which is a
/// config bug rather than an empty value, so it is reported instead of
/// rendering nothing.
///
/// # Errors
/// See [`TemplateError`].
pub fn render<F>(template: &str, lookup: F) -> Result<String, TemplateError>
where
    F: Fn(&str) -> Option<String>,
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    loop {
        let Some(open_at) = rest.find("{{") else {
            out.push_str(rest);
            return Ok(out);
        };

        let (literal, from_open) = rest.split_at(open_at);
        out.push_str(literal);

        let after_open = from_open.get(2..).unwrap_or_default();
        let Some(close_at) = after_open.find("}}") else {
            return Err(TemplateError::Unclosed {
                template: template.to_owned(),
            });
        };

        let (placeholder, from_close) = after_open.split_at(close_at);
        out.push_str(&resolve(placeholder, &lookup)?);
        rest = from_close.get(2..).unwrap_or_default();
    }
}

/// Resolves one placeholder body (the text between `{{` and `}}`).
fn resolve<F>(placeholder: &str, lookup: &F) -> Result<String, TemplateError>
where
    F: Fn(&str) -> Option<String>,
{
    let malformed = || TemplateError::Malformed {
        placeholder: placeholder.to_owned(),
    };

    let (transform_name, after_paren) = placeholder.trim().split_once('(').ok_or_else(malformed)?;
    let group = after_paren.strip_suffix(')').ok_or_else(malformed)?.trim();
    let transform_name = transform_name.trim();

    let transform =
        CaseTransform::parse(transform_name).ok_or_else(|| TemplateError::UnknownTransform {
            name: transform_name.to_owned(),
            available: CaseTransform::ALL.map(CaseTransform::name).join(", "),
        })?;

    let value = lookup(group).ok_or_else(|| TemplateError::UnknownGroup {
        group: group.to_owned(),
    })?;

    Ok(transform.apply(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolves `name` to `create-client` and nothing else, which is the shape
    /// a `naming` rule's capture groups actually have.
    fn captures(group: &str) -> Option<String> {
        match group {
            "name" => Some("create-client".to_owned()),
            "suffix" => Some("use-case".to_owned()),
            _ => None,
        }
    }

    /// The motivating case from docs/CONFIG.md: a kebab-case filename stem
    /// dictates a `PascalCase` export.
    #[test]
    fn pascal_turns_a_kebab_stem_into_a_pascal_symbol() {
        let out = render("{{pascal(name)}}", captures).expect("should render");
        assert_eq!(out, "CreateClient");
    }

    #[test]
    fn every_transform_produces_its_documented_shape() {
        let cases = [
            ("{{pascal(name)}}", "CreateClient"),
            ("{{camel(name)}}", "createClient"),
            ("{{kebab(name)}}", "create-client"),
            ("{{snake(name)}}", "create_client"),
            ("{{upper(name)}}", "CREATE-CLIENT"),
            ("{{lower(name)}}", "create-client"),
            ("{{raw(name)}}", "create-client"),
        ];
        for (template, expected) in cases {
            let out = render(template, captures).expect("should render");
            assert_eq!(out, expected, "template {template}");
        }
    }

    /// `kebab` and `snake` have to actually split a `PascalCase` input, not just
    /// lowercase it.
    #[test]
    fn kebab_and_snake_split_pascal_input() {
        let pascal = |g: &str| (g == "n").then(|| "CreateClient".to_owned());
        assert_eq!(
            render("{{kebab(n)}}", pascal).expect("renders"),
            "create-client"
        );
        assert_eq!(
            render("{{snake(n)}}", pascal).expect("renders"),
            "create_client"
        );
    }

    /// Literal text around a placeholder survives. `signature_hint` relies on
    /// this to emit a realistic type signature.
    #[test]
    fn literal_text_around_a_placeholder_is_preserved() {
        let out = render("(deps: {{pascal(name)}}Deps)", captures).expect("should render");
        assert_eq!(out, "(deps: CreateClientDeps)");
    }

    /// A group may be referenced repeatedly, and by different transforms.
    #[test]
    fn a_group_may_be_used_more_than_once() {
        let out = render(
            "{{pascal(name)}}<{{pascal(name)}}Input, {{camel(suffix)}}>",
            captures,
        )
        .expect("should render");
        assert_eq!(out, "CreateClient<CreateClientInput, useCase>");
    }

    #[test]
    fn a_template_with_no_placeholder_is_returned_unchanged() {
        let out = render("PlainName", captures).expect("should render");
        assert_eq!(out, "PlainName");
    }

    /// The error names the valid transforms, so a typo is self-correcting
    /// without opening the docs.
    #[test]
    fn an_unknown_transform_lists_the_valid_ones() {
        // Asserting the whole error pins the exact sentence a user reads,
        // rather than just checking that some substring is in there.
        assert_eq!(
            render("{{pascalcase(name)}}", captures),
            Err(TemplateError::UnknownTransform {
                name: "pascalcase".to_owned(),
                available: "pascal, camel, kebab, snake, upper, lower, raw".to_owned(),
            })
        );
    }

    /// Referencing a group the regex does not define is a config bug, and it
    /// should say which group rather than silently rendering nothing.
    #[test]
    fn an_unknown_capture_group_is_named_in_the_error() {
        let err = render("{{pascal(nome)}}", captures).expect_err("should fail");
        assert_eq!(
            err,
            TemplateError::UnknownGroup {
                group: "nome".to_owned()
            }
        );
    }

    #[test]
    fn an_unclosed_placeholder_is_rejected() {
        let err = render("{{pascal(name)", captures).expect_err("should fail");
        assert!(matches!(err, TemplateError::Unclosed { .. }), "got {err:?}");
    }

    #[test]
    fn a_placeholder_without_the_call_form_is_rejected() {
        let err = render("{{name}}", captures).expect_err("should fail");
        assert!(
            matches!(err, TemplateError::Malformed { .. }),
            "got {err:?}"
        );
    }

    /// `Display` is what puts a transform into an error message, so it has to
    /// be the same spelling a user would type back into the config.
    #[test]
    fn display_renders_the_name_used_in_config() {
        assert_eq!(CaseTransform::Pascal.to_string(), "pascal");
        assert_eq!(CaseTransform::Raw.to_string(), "raw");
        for transform in CaseTransform::ALL {
            assert_eq!(transform.to_string(), transform.name());
            assert!(!transform.to_string().is_empty());
        }
    }

    #[test]
    fn transform_names_round_trip_through_parse() {
        for transform in CaseTransform::ALL {
            assert_eq!(CaseTransform::parse(transform.name()), Some(transform));
        }
        assert_eq!(CaseTransform::parse("nope"), None);
    }
}
