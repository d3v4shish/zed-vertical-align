use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatScope {
    Document,
    Lines { start: usize, end: usize },
}

#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    row: usize,
    start: usize,
    text: &'a str,
}

#[derive(Debug, Clone)]
struct CppConstructorInitializers {
    end_row: usize,
    range_end: usize,
    entries: Vec<String>,
}

#[derive(Debug, Clone)]
struct Separator {
    kind: &'static str,
    range: Range<usize>,
    whitespace_start: usize,
    trailing_whitespace_before_terminator: Option<Range<usize>>,
}

#[derive(Debug, Clone)]
struct Candidate {
    row: usize,
    indent: usize,
    separator: Separator,
}

#[derive(Debug, Clone)]
struct PrintLabelCandidate {
    row: usize,
    indent: usize,
    delimiter: u8,
    padding_range: Range<usize>,
    label_width: usize,
}

#[derive(Default)]
struct MultilineState {
    block_comment: bool,
    quote: Option<u8>,
    triple_quote: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
struct CodeBlock {
    start: usize,
    end: usize,
    do_while: bool,
}

struct StructuralInfo {
    blocks: Vec<CodeBlock>,
    required_before: Vec<bool>,
    import_rows: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutMode {
    Canonical,
    PreserveNativeContinuation,
}

/// Plans non-overlapping whitespace and declaration-reflow edits for a document.
/// Compatible rows are aligned only within their own contiguous blocks.
pub fn format_document(text: &str, language_id: &str, tab_size: usize) -> Vec<TextEdit> {
    format(
        text,
        language_id,
        tab_size,
        FormatScope::Document,
        LayoutMode::Canonical,
    )
}

/// Formats a complete document and returns the resulting text.
///
/// The LSP uses this after an optional language-native formatter has produced a
/// base layout. Keeping this operation in the pure core makes the fallback
/// deterministic and lets the LSP compose the external and structural passes
/// into one edit for Zed.
pub fn format_document_text(text: &str, language_id: &str, tab_size: usize) -> String {
    apply_text_edits(text, &format_document(text, language_id, tab_size))
}

/// Formats text already produced by a language-native formatter.
///
/// Native continuation indentation is retained while this core still enforces
/// Zed Vertical Align's declaration, access-section, spacing, and alignment
/// rules.
pub fn format_document_text_after_native(text: &str, language_id: &str, tab_size: usize) -> String {
    apply_text_edits(
        text,
        &format(
            text,
            language_id,
            tab_size,
            FormatScope::Document,
            LayoutMode::PreserveNativeContinuation,
        ),
    )
}

/// Plans edits only for compatible blocks intersecting the inclusive line range.
pub fn format_range(
    text: &str,
    language_id: &str,
    tab_size: usize,
    start: usize,
    end: usize,
) -> Vec<TextEdit> {
    format(
        text,
        language_id,
        tab_size,
        FormatScope::Lines { start, end },
        LayoutMode::Canonical,
    )
}

fn format(
    text: &str,
    language_id: &str,
    tab_size: usize,
    scope: FormatScope,
    layout_mode: LayoutMode,
) -> Vec<TextEdit> {
    let original_lines = lines(text);
    if original_lines.is_empty() {
        return Vec::new();
    }

    let layout_edits = structural_layout_edits(
        text,
        &original_lines,
        language_id,
        tab_size,
        scope,
        layout_mode,
    );
    let laid_out_text = apply_text_edits(text, &layout_edits);
    let scope = remap_scope(text, &original_lines, &laid_out_text, &layout_edits, scope);
    let laid_out_lines = lines(&laid_out_text);

    let spacing_edits =
        structural_spacing_edits(&laid_out_text, &laid_out_lines, language_id, scope);
    let spaced_text = apply_text_edits(&laid_out_text, &spacing_edits);
    let scope = remap_scope(
        &laid_out_text,
        &laid_out_lines,
        &spaced_text,
        &spacing_edits,
        scope,
    );
    let lines = lines(&spaced_text);

    let excluded_rows = multiline_excluded_rows(&lines, language_id);
    let (mut reflowed_rows, mut edits) = reflow_signatures(
        &spaced_text,
        &lines,
        &excluded_rows,
        tab_size,
        language_id,
        scope,
    );
    let (python_keyword_rows, python_keyword_edits) = reflow_python_keyword_calls(
        &spaced_text,
        &lines,
        &excluded_rows,
        &reflowed_rows,
        tab_size,
        language_id,
        scope,
    );
    for (row, handled) in python_keyword_rows.into_iter().enumerate() {
        reflowed_rows[row] |= handled;
    }
    edits.extend(python_keyword_edits);
    let mut profile_alignment_rows =
        c_allman_initialized_declaration_alignment_edits(&lines, language_id, tab_size, &mut edits);
    for (row, handled) in
        python_annotation_alignment_edits(&lines, language_id, tab_size, &mut edits)
            .into_iter()
            .enumerate()
    {
        profile_alignment_rows[row] |= handled;
    }

    let candidates = lines
        .iter()
        .zip(&excluded_rows)
        .map(|(line, excluded)| {
            (!*excluded && !reflowed_rows[line.row] && !profile_alignment_rows[line.row])
                .then(|| find_alignment_separator(line.text, language_id))
                .flatten()
                .map(|separator| Candidate {
                    row: line.row,
                    indent: alignment_group_indent(line.text, &separator),
                    separator,
                })
        })
        .collect::<Vec<_>>();

    for group in contiguous_groups(&candidates) {
        if group.len() < 2 || !group_intersects_scope(&group, scope) {
            continue;
        }
        edits.extend(alignment_edits_for_group(&lines, &group, tab_size));
    }

    edits.extend(print_label_alignment_edits(
        &lines,
        &excluded_rows,
        language_id,
        tab_size,
        scope,
    ));

    let formatted_text = apply_text_edits(&spaced_text, &edits);
    text_edit_for_replacement(text, formatted_text)
        .into_iter()
        .collect()
}

#[derive(Debug)]
struct InitializedCDeclaration {
    row: usize,
    indent: usize,
    type_end: usize,
    name_start: usize,
    name_end: usize,
    assignment_start: usize,
}

fn c_allman_initialized_declaration_alignment_edits(
    lines: &[Line<'_>],
    language_id: &str,
    tab_size: usize,
    edits: &mut Vec<TextEdit>,
) -> Vec<bool> {
    let mut handled_rows = vec![false; lines.len()];
    if !is_c_allman_language(language_id) {
        return handled_rows;
    }

    let declarations = lines
        .iter()
        .filter_map(|line| {
            parse_initialized_c_declaration(line).map(|declaration| (line, declaration))
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::<Vec<(&Line<'_>, InitializedCDeclaration)>>::new();
    let mut current = Vec::<(&Line<'_>, InitializedCDeclaration)>::new();

    for (line, declaration) in declarations {
        let continues = current.last().is_some_and(|(previous_line, previous)| {
            previous.indent == declaration.indent
                && lines[previous_line.row + 1..line.row]
                    .iter()
                    .all(|between| between.text.trim().is_empty())
        });
        if !continues && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push((line, declaration));
    }
    if !current.is_empty() {
        groups.push(current);
    }

    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let name_column = group
            .iter()
            .map(|(line, declaration)| {
                logical_column(&line.text[..declaration.type_end], tab_size) + 1
            })
            .max()
            .unwrap_or_default();
        let assignment_column = group
            .iter()
            .map(|(line, declaration)| {
                name_column
                    + logical_column(
                        &line.text[declaration.name_start..declaration.name_end],
                        tab_size,
                    )
                    + 1
            })
            .max()
            .unwrap_or_default();
        for (line, declaration) in group {
            handled_rows[declaration.row] = true;
            let type_column = logical_column(&line.text[..declaration.type_end], tab_size);
            let type_gap = " ".repeat(name_column.saturating_sub(type_column));
            edits.push(TextEdit {
                range: line.start + declaration.type_end..line.start + declaration.name_start,
                replacement: type_gap,
            });

            let name_width = logical_column(
                &line.text[declaration.name_start..declaration.name_end],
                tab_size,
            );
            let assignment_gap =
                " ".repeat(assignment_column.saturating_sub(name_column + name_width));
            edits.push(TextEdit {
                range: line.start + declaration.name_end..line.start + declaration.assignment_start,
                replacement: assignment_gap,
            });
        }
    }
    handled_rows
}

fn parse_initialized_c_declaration(line: &Line<'_>) -> Option<InitializedCDeclaration> {
    let separator = find_separator(line.text)?;
    if separator.kind != "="
        || line.text.trim_start().starts_with('.')
        || !line.text.trim_end().ends_with(';')
    {
        return None;
    }
    let lhs = line.text[..separator.whitespace_start].trim_end();
    if lhs.contains(['(', ')', '[', ']', '.', '>']) {
        return None;
    }
    let name_start = last_identifier_start(lhs)?;
    let before_name = &lhs[..name_start];
    let type_end = before_name.trim_end().len();
    let type_prefix = &lhs[..type_end];
    if type_prefix.is_empty()
        || is_c_like_statement(type_prefix)
        || !(before_name.ends_with(char::is_whitespace)
            || type_prefix.ends_with('*')
            || type_prefix.ends_with('&'))
    {
        return None;
    }
    Some(InitializedCDeclaration {
        row: line.row,
        indent: indentation(line.text),
        type_end,
        name_start,
        name_end: name_start + lhs[name_start..].len(),
        assignment_start: separator.range.start,
    })
}

#[derive(Debug)]
struct PythonAnnotation {
    row: usize,
    indent: usize,
    name_end: usize,
    colon_start: usize,
    type_start: usize,
    type_end: usize,
    assignment_start: usize,
}

fn python_annotation_alignment_edits(
    lines: &[Line<'_>],
    language_id: &str,
    tab_size: usize,
    edits: &mut Vec<TextEdit>,
) -> Vec<bool> {
    let mut handled_rows = vec![false; lines.len()];
    if !language_id.eq_ignore_ascii_case("python") {
        return handled_rows;
    }

    let mut groups = Vec::<Vec<(&Line<'_>, PythonAnnotation)>>::new();
    let mut current = Vec::<(&Line<'_>, PythonAnnotation)>::new();
    for line in lines {
        let Some(annotation) = parse_python_annotation(line) else {
            if !line.text.trim().is_empty() && !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
            continue;
        };
        let continues = current.last().is_some_and(|(previous_line, previous)| {
            previous.indent == annotation.indent && previous_line.row + 1 == line.row
        });
        if !continues && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push((line, annotation));
    }
    if !current.is_empty() {
        groups.push(current);
    }

    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let colon_column = group
            .iter()
            .map(|(line, annotation)| {
                logical_column(&line.text[..annotation.name_end], tab_size) + 1
            })
            .max()
            .unwrap_or_default();
        let assignment_column = group
            .iter()
            .map(|(line, annotation)| {
                colon_column
                    + 2
                    + logical_column(
                        &line.text[annotation.type_start..annotation.type_end],
                        tab_size,
                    )
                    + 1
            })
            .max()
            .unwrap_or_default();
        for (line, annotation) in group {
            handled_rows[annotation.row] = true;
            let name_column = logical_column(&line.text[..annotation.name_end], tab_size);
            edits.push(TextEdit {
                range: line.start + annotation.name_end..line.start + annotation.colon_start,
                replacement: " ".repeat(colon_column.saturating_sub(name_column)),
            });
            edits.push(TextEdit {
                range: line.start + annotation.colon_start + 1..line.start + annotation.type_start,
                replacement: " ".to_string(),
            });
            let type_width = logical_column(
                &line.text[annotation.type_start..annotation.type_end],
                tab_size,
            );
            edits.push(TextEdit {
                range: line.start + annotation.type_end..line.start + annotation.assignment_start,
                replacement: " "
                    .repeat(assignment_column.saturating_sub(colon_column + 2 + type_width)),
            });
        }
    }
    handled_rows
}

fn parse_python_annotation(line: &Line<'_>) -> Option<PythonAnnotation> {
    let code = line.text.split('#').next()?.trim_end();
    let colon_start = code.find(':')?;
    let assignment_start = code[colon_start + 1..]
        .find('=')
        .map(|offset| colon_start + 1 + offset)?;
    if code[assignment_start..].starts_with("==") {
        return None;
    }
    let name = code[..colon_start].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }
    let name_end = code[..colon_start].trim_end().len();
    let type_start = colon_start
        + 1
        + code[colon_start + 1..]
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map(|(offset, _)| offset)?;
    let type_end = code[..assignment_start].trim_end().len();
    (type_start < type_end).then_some(PythonAnnotation {
        row: line.row,
        indent: indentation(line.text),
        name_end,
        colon_start,
        type_start,
        type_end,
        assignment_start,
    })
}

fn apply_text_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut edits = edits.to_vec();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
    let mut result = text.to_string();
    for edit in edits {
        result.replace_range(edit.range, &edit.replacement);
    }
    result
}

fn text_edit_for_replacement(text: &str, replacement: String) -> Option<TextEdit> {
    (replacement != text).then_some(TextEdit {
        range: 0..text.len(),
        replacement,
    })
}

fn remap_scope(
    text: &str,
    source_lines: &[Line<'_>],
    replacement: &str,
    edits: &[TextEdit],
    scope: FormatScope,
) -> FormatScope {
    let FormatScope::Lines { start, end } = scope else {
        return scope;
    };
    let Some(first) = source_lines.get(start) else {
        return scope;
    };
    let end_offset = source_lines
        .get(end.saturating_add(1))
        .map_or(text.len(), |line| line.start);
    let start_offset = map_offset(first.start, edits, false);
    let end_offset = map_offset(end_offset, edits, true);
    let replacement_lines = lines(replacement);
    let start = line_for_offset(&replacement_lines, start_offset);
    let end = line_for_offset(&replacement_lines, end_offset.saturating_sub(1)).max(start);
    FormatScope::Lines { start, end }
}

fn map_offset(offset: usize, edits: &[TextEdit], end_affinity: bool) -> usize {
    let mut delta = 0isize;
    let mut edits = edits.to_vec();
    edits.sort_by_key(|edit| edit.range.start);
    for edit in edits {
        if offset < edit.range.start {
            break;
        }
        if offset <= edit.range.end {
            return (edit.range.start as isize
                + delta
                + if end_affinity {
                    edit.replacement.len() as isize
                } else {
                    0
                })
            .max(0) as usize;
        }
        delta += edit.replacement.len() as isize - edit.range.len() as isize;
    }
    (offset as isize + delta).max(0) as usize
}

fn line_for_offset(lines: &[Line<'_>], offset: usize) -> usize {
    lines
        .iter()
        .rposition(|line| line.start <= offset)
        .unwrap_or_default()
}

fn alignment_group_indent(line: &str, separator: &Separator) -> usize {
    if separator.kind == "cpp_initializer" {
        // A conventional C++ initializer list indents its first ':' row two columns
        // less than the following member rows. They are nevertheless one block.
        0
    } else {
        indentation(line)
    }
}

fn group_intersects_scope(group: &[Candidate], scope: FormatScope) -> bool {
    match scope {
        FormatScope::Document => true,
        FormatScope::Lines { start, end } => group
            .iter()
            .any(|candidate| (start..=end).contains(&candidate.row)),
    }
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut start = 0;
    let mut result = Vec::new();

    for (row, raw_line) in text.split_inclusive('\n').enumerate() {
        let text = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let text = text.strip_suffix('\r').unwrap_or(text);
        result.push(Line { row, start, text });
        start += raw_line.len();
    }

    if text.is_empty() || !text.ends_with('\n') {
        if result.is_empty() {
            result.push(Line {
                row: 0,
                start: 0,
                text,
            });
        }
    }

    result
}

#[derive(Debug, Clone, Copy)]
enum LayoutBrace {
    Other,
    CppClass { has_access_section: bool },
}

fn structural_layout_edits(
    text: &str,
    source_lines: &[Line<'_>],
    language_id: &str,
    tab_size: usize,
    scope: FormatScope,
    layout_mode: LayoutMode,
) -> Vec<TextEdit> {
    let normalized = if is_c_allman_language(language_id) && matches!(scope, FormatScope::Document)
    {
        normalize_c_allman_declaration_lines(text, language_id, tab_size)
    } else {
        text.to_string()
    };
    let lines = lines(&normalized);
    let info = structural_info(&lines, language_id);
    let active_rows = if normalized == text {
        structural_active_rows(source_lines.len(), &info.blocks, scope)
    } else {
        vec![true; lines.len()]
    };
    let replacement = if language_id.eq_ignore_ascii_case("python") {
        layout_python(&normalized, &lines, &active_rows, tab_size)
    } else {
        layout_c_like(
            &normalized,
            &lines,
            &active_rows,
            language_id,
            tab_size,
            layout_mode,
        )
    };

    text_edit_for_replacement(text, replacement)
        .into_iter()
        .collect()
}

fn layout_c_like(
    text: &str,
    lines: &[Line<'_>],
    active_rows: &[bool],
    language_id: &str,
    tab_size: usize,
    layout_mode: LayoutMode,
) -> String {
    let line_ending = line_ending(text);
    let trailing_line_ending = text.ends_with(line_ending);
    let mut result = Vec::with_capacity(lines.len());
    let mut scan_state = CLikeScanState::default();
    let mut braces = Vec::new();
    let mut continuations = 0usize;
    let mut previous_source = String::new();

    for line in lines {
        let source = line.text;
        let source_trimmed = source.trim();
        let code = c_like_code(source, &mut scan_state, language_id);
        let code_trimmed = code.trim();
        let active = active_rows.get(line.row).copied().unwrap_or(true);

        if source_trimmed.is_empty() {
            result.push(String::new());
            continue;
        }

        if !active {
            result.push(source.to_string());
            update_layout_braces(&code, &previous_source, language_id, &mut braces);
            update_continuation_depth(&code, &mut continuations);
            if !source_trimmed.is_empty() {
                previous_source = source_trimmed.to_string();
            }
            continue;
        }

        if source_trimmed.starts_with('#') {
            result.push(source_trimmed.to_string());
            continue;
        }

        let leading_closes = leading_close_count(&code);
        let depth = braces.len().saturating_sub(leading_closes);
        let access_label = is_cpp_language(language_id) && is_cpp_access_label(source_trimmed);
        let closing_delimiter = matches!(code.trim_start().chars().next(), Some(')' | ']'));
        let continuation_extra = usize::from(
            (continuations > 0 && !closing_delimiter)
                || (leading_closes == 0
                    && is_expression_line_continuation(source_trimmed, &previous_source)),
        );
        let needs_structural_indent = !braces.is_empty()
            || leading_closes > 0
            || continuation_extra > 0
            || is_declaration_header(source_trimmed, language_id)
            || is_c_like_control_header(source_trimmed);
        if !needs_structural_indent {
            result.push(source.to_string());
            update_layout_braces(&code, &previous_source, language_id, &mut braces);
            update_continuation_depth(&code, &mut continuations);
            if !source_trimmed.is_empty() {
                previous_source = source_trimmed.to_string();
            }
            continue;
        }
        let access_extra = if access_label {
            0
        } else {
            cpp_access_extra(&braces, depth, language_id)
        };
        let desired_indent = tab_size.max(1) * (depth + access_extra + continuation_extra);
        let existing_indent = logical_column(&source[..indentation(source)], tab_size.max(1));
        let preserve_root_continuation = braces.is_empty()
            && continuation_extra > 0
            && !is_declaration_header(source_trimmed, language_id)
            && !is_c_like_control_header(source_trimmed);
        let indent_width = if continuation_extra > 0
            && (layout_mode == LayoutMode::PreserveNativeContinuation || preserve_root_continuation)
        {
            desired_indent.max(existing_indent)
        } else {
            desired_indent
        };
        let indent = " ".repeat(indent_width);
        let content = if code_trimmed.is_empty() {
            source_trimmed
        } else {
            source_trimmed
        };
        result.push(format!("{indent}{content}"));

        if access_label {
            mark_cpp_access_section(&mut braces);
        }
        update_layout_braces(&code, &previous_source, language_id, &mut braces);
        update_continuation_depth(&code, &mut continuations);
        if !source_trimmed.is_empty() {
            previous_source = source_trimmed.to_string();
        }
    }

    join_lines(result, line_ending, trailing_line_ending)
}

fn is_expression_line_continuation(text: &str, previous_code: &str) -> bool {
    text.starts_with("<<")
        || text.starts_with(">>")
        || (text.starts_with('.') && !is_designated_initializer_row(text))
        || text.starts_with("->")
        || (!previous_code.starts_with("//")
            && !previous_code.starts_with("/*")
            && !is_cpp_access_label(previous_code)
            && previous_code.ends_with(['=', '+', '-', '*', '/', '?', '.']))
}

fn is_designated_initializer_row(text: &str) -> bool {
    text.starts_with('.')
        && text
            .find('=')
            .is_some_and(|assignment| assignment > 1 && !text[..assignment].contains('('))
}

fn layout_python(text: &str, lines: &[Line<'_>], active_rows: &[bool], tab_size: usize) -> String {
    let line_ending = line_ending(text);
    let trailing_line_ending = text.ends_with(line_ending);
    let mut result = Vec::with_capacity(lines.len());
    let mut suites = Vec::<(usize, bool)>::new();
    let mut delimiters = Vec::<usize>::new();

    for line in lines {
        let source = line.text;
        let source_trimmed = source.trim();
        if source_trimmed.is_empty() {
            result.push(String::new());
            continue;
        }

        let code = python_code(source);
        let code_trimmed = code.trim();
        let original_indent = logical_column(&source[..indentation(source)], tab_size.max(1));
        let inside_delimiters = !delimiters.is_empty();
        let leading_closes = leading_python_delimiter_closes(code_trimmed);
        if inside_delimiters {
            let indent_width = if leading_closes > 0 {
                delimiters
                    .get(delimiters.len().saturating_sub(leading_closes))
                    .copied()
                    .unwrap_or(original_indent)
            } else {
                delimiters.last().copied().unwrap_or(original_indent) + tab_size.max(1)
            };
            let active = active_rows.get(line.row).copied().unwrap_or(true);
            if active {
                result.push(format!("{}{}", " ".repeat(indent_width), source_trimmed));
            } else {
                result.push(source.to_string());
            }
            update_python_delimiters(&code, indent_width, &mut delimiters);
            continue;
        }
        let continuation = is_python_continuation(code_trimmed);
        while let Some((suite_indent, has_body)) = suites.last().copied() {
            let infer_first_body = original_indent == suite_indent
                && !has_body
                && !continuation
                && !is_python_declaration(code_trimmed);
            if original_indent > suite_indent || infer_first_body {
                break;
            }
            suites.pop();
        }

        if let Some((_, has_body)) = suites.last_mut() {
            *has_body = true;
        }

        let active = active_rows.get(line.row).copied().unwrap_or(true);
        let indent_width = tab_size.max(1) * suites.len();
        if active {
            let indent = " ".repeat(indent_width);
            result.push(format!("{indent}{source_trimmed}"));
        } else {
            result.push(source.to_string());
        }

        update_python_delimiters(&code, indent_width, &mut delimiters);

        if !code_trimmed.is_empty() && is_python_suite(code_trimmed) {
            suites.push((original_indent, false));
        }
    }

    join_lines(result, line_ending, trailing_line_ending)
}

fn leading_python_delimiter_closes(code: &str) -> usize {
    code.trim_start()
        .chars()
        .take_while(|character| matches!(character, ')' | ']' | '}'))
        .count()
}

fn update_python_delimiters(code: &str, indent: usize, delimiters: &mut Vec<usize>) {
    for character in code.chars() {
        match character {
            '(' | '[' | '{' => delimiters.push(indent),
            ')' | ']' | '}' => {
                delimiters.pop();
            }
            _ => {}
        }
    }
}

fn is_python_declaration(text: &str) -> bool {
    text.starts_with("def ") || text.starts_with("class ") || text.starts_with('@')
}

fn line_ending(text: &str) -> &str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn join_lines(lines: Vec<String>, line_ending: &str, trailing_line_ending: bool) -> String {
    let mut result = lines.join(line_ending);
    if trailing_line_ending {
        result.push_str(line_ending);
    }
    result
}

fn leading_close_count(code: &str) -> usize {
    code.trim_start()
        .chars()
        .take_while(|character| *character == '}')
        .count()
}

fn is_cpp_access_label(text: &str) -> bool {
    matches!(text, "public:" | "private:" | "protected:")
}

fn cpp_access_extra(braces: &[LayoutBrace], depth: usize, language_id: &str) -> usize {
    if !is_cpp_language(language_id) {
        return 0;
    }
    braces
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, brace)| match brace {
            LayoutBrace::CppClass {
                has_access_section: true,
            } if depth >= index + 1 => Some(1),
            _ => None,
        })
        .unwrap_or(0)
}

fn mark_cpp_access_section(braces: &mut [LayoutBrace]) {
    if let Some(LayoutBrace::CppClass { has_access_section }) = braces
        .iter_mut()
        .rev()
        .find(|brace| matches!(brace, LayoutBrace::CppClass { .. }))
    {
        *has_access_section = true;
    }
}

fn update_layout_braces(
    code: &str,
    previous_code: &str,
    language_id: &str,
    braces: &mut Vec<LayoutBrace>,
) {
    let bytes = code.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b'{' => {
                let prefix = &code[..offset];
                let context = if prefix.trim().is_empty() {
                    previous_code
                } else {
                    prefix
                };
                let class = is_cpp_language(language_id)
                    && ["class", "struct", "union"]
                        .into_iter()
                        .any(|word| contains_word(context, word));
                braces.push(if class {
                    LayoutBrace::CppClass {
                        has_access_section: false,
                    }
                } else {
                    LayoutBrace::Other
                });
                offset += 1;
            }
            b'}' => {
                braces.pop();
                offset += 1;
            }
            _ => offset += char_len(code, offset),
        }
    }
}

fn update_continuation_depth(code: &str, depth: &mut usize) {
    for character in code.chars() {
        match character {
            '(' | '[' => *depth += 1,
            ')' | ']' => *depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

fn normalize_c_allman_declaration_lines(text: &str, language_id: &str, tab_size: usize) -> String {
    let line_ending = line_ending(text);
    let trailing_line_ending = text.ends_with(line_ending);
    let raw_lines = if trailing_line_ending {
        &text[..text.len().saturating_sub(line_ending.len())]
    } else {
        text
    };
    let mut result = Vec::<String>::new();
    let mut designated_initializer_indent = None::<String>;
    let mut sources = raw_lines.split(line_ending).peekable();

    while let Some(source) = sources.next() {
        let trimmed = source.trim();
        let indent = &source[..source.len() - source.trim_start().len()];
        if let Some(header_indent) = &designated_initializer_indent {
            if let Some(last_field) = trimmed.strip_suffix("};") {
                let last_field = last_field.trim();
                if !last_field.is_empty() {
                    result.push(format!(
                        "{header_indent}{}{last_field}",
                        " ".repeat(tab_size.max(1))
                    ));
                }
                result.push(format!("{header_indent}}};"));
                designated_initializer_indent = None;
            } else {
                result.push(format!(
                    "{header_indent}{}{trimmed}",
                    " ".repeat(tab_size.max(1))
                ));
            }
            continue;
        }
        if is_c_allman_language(language_id) && trimmed.ends_with('=') {
            if let Some(next) = sources.peek().copied() {
                let continuation = next.trim();
                if continuation.ends_with(';')
                    && !continuation.starts_with(['.', '/', '*'])
                    && !continuation.contains('{')
                {
                    result.push(format!("{} {continuation}", source.trim_end()));
                    sources.next();
                    continue;
                }
            }
        }
        if is_cpp_language(language_id) {
            if let Some((template, declaration)) = split_inline_cpp_template(trimmed) {
                result.push(format!("{indent}{template}"));
                result.push(format!("{indent}{declaration}"));
                continue;
            }
        }
        let source = if is_cpp_language(language_id) {
            normalize_cpp_template_spacing(source)
        } else {
            source.to_string()
        };
        if is_cpp_language(language_id) {
            if let Some(expanded) = expand_inline_designated_initializer(&source, tab_size) {
                result.extend(expanded);
                continue;
            }
            if let Some((header, first_field)) =
                split_multiline_designated_initializer_start(&source)
            {
                let field_indent = format!("{indent}{}", " ".repeat(tab_size.max(1)));
                result.push(format!("{indent}{header} {{"));
                result.push(format!("{field_indent}{first_field}"));
                designated_initializer_indent = Some(indent.to_string());
                continue;
            }
            if let Some(expanded) = split_cpp_stream_head(&source, tab_size) {
                result.extend(expanded);
                continue;
            }
        }
        if let Some(header) = split_allman_header(&source, language_id) {
            result.push(header);
            result.push(indent.to_string() + "{");
            continue;
        }
        result.push(source);
    }
    join_lines(result, line_ending, trailing_line_ending)
}

fn normalize_cpp_template_spacing(line: &str) -> String {
    let trimmed_start = line.trim_start();
    if let Some(rest) = trimmed_start.strip_prefix("template<") {
        let indent = &line[..line.len() - trimmed_start.len()];
        format!("{indent}template <{rest}")
    } else {
        line.to_string()
    }
}

fn split_inline_cpp_template(line: &str) -> Option<(String, String)> {
    let line = normalize_cpp_template_spacing(line);
    let template = line.strip_prefix("template ")?;
    let open = template.find('<')?;
    let mut depth = 0usize;
    let mut end = None;
    for (offset, character) in template[open..].char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    end = Some(open + offset + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let declaration = template[end..].trim_start();
    if ["class ", "struct ", "union ", "enum "]
        .into_iter()
        .any(|prefix| declaration.starts_with(prefix))
    {
        Some((
            format!("template {}", &template[..end]),
            declaration.to_string(),
        ))
    } else {
        None
    }
}

fn split_allman_header(line: &str, language_id: &str) -> Option<String> {
    let trimmed = line.trim();
    let header = trimmed.strip_suffix('{')?.trim_end();
    if header.is_empty() || is_c_like_control_header(header) || header.starts_with("else") {
        return None;
    }
    let is_record = ["class ", "struct ", "union ", "enum "]
        .into_iter()
        .any(|prefix| header.starts_with(prefix));
    let is_function = header.ends_with(')')
        || header.ends_with(" const")
        || header.ends_with(" noexcept")
        || header.ends_with(" override")
        || header.ends_with(" final");
    (is_record || is_function || (is_cpp_language(language_id) && header.starts_with("namespace ")))
        .then(|| {
            let indent = &line[..line.len() - line.trim_start().len()];
            format!("{indent}{header}")
        })
}

fn expand_inline_designated_initializer(line: &str, tab_size: usize) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let open = trimmed.find('{')?;
    let body = trimmed.get(open + 1..)?.strip_suffix("};")?.trim();
    if !body.starts_with('.') || trimmed[..open].trim().is_empty() {
        return None;
    }
    let fields = split_top_level_commas(body);
    if fields.len() < 2
        || fields
            .iter()
            .any(|field| !field.trim_start().starts_with('.'))
    {
        return None;
    }
    let indent = &line[..line.len() - line.trim_start().len()];
    let header = trimmed[..open].trim_end();
    let field_indent = format!("{indent}{}", " ".repeat(tab_size.max(1)));
    let mut result = Vec::with_capacity(fields.len() + 2);
    result.push(format!("{indent}{header} {{"));
    for (index, field) in fields.iter().enumerate() {
        let comma = (index + 1 < fields.len()).then_some(",").unwrap_or("");
        result.push(format!(
            "{field_indent}{}{comma}",
            field.trim().trim_end_matches(',')
        ));
    }
    result.push(format!("{indent}}};"));
    Some(result)
}

fn split_multiline_designated_initializer_start(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let open = trimmed.find('{')?;
    let header = trimmed[..open].trim_end();
    let first_field = trimmed[open + 1..].trim();
    (!header.is_empty() && first_field.starts_with('.') && !first_field.ends_with("};"))
        .then_some((header, first_field))
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in text.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                result.push(&text[start..offset]);
                start = offset + 1;
            }
            _ => {}
        }
    }
    result.push(&text[start..]);
    result
}

fn split_cpp_stream_head(line: &str, tab_size: usize) -> Option<Vec<String>> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("std::cout")?.trim_start();
    if !rest.starts_with("<<") {
        return None;
    }
    let indent = &line[..line.len() - trimmed.len()];
    Some(vec![
        format!("{indent}std::cout"),
        format!("{indent}{} {rest}", " ".repeat(tab_size.max(1))),
    ])
}

fn structural_spacing_edits(
    text: &str,
    lines: &[Line<'_>],
    language_id: &str,
    scope: FormatScope,
) -> Vec<TextEdit> {
    let info = structural_info(lines, language_id);
    let active_rows = structural_active_rows(lines.len(), &info.blocks, scope);
    let significant_rows = lines
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .map(|line| line.row)
        .collect::<Vec<_>>();
    let Some(&first) = significant_rows.first() else {
        return Vec::new();
    };
    let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut edits = Vec::new();

    if first > 0 && row_is_active(first, &active_rows, scope) {
        edits.push(TextEdit {
            range: 0..lines[first].start,
            replacement: String::new(),
        });
    }

    for rows in significant_rows.windows(2) {
        let previous = rows[0];
        let next = rows[1];
        if !gap_is_active(previous, next, &active_rows, scope) {
            continue;
        }
        let needs_separator =
            info.required_before[next] || (info.import_rows[previous] && !info.import_rows[next]);
        let range = lines[previous].start + lines[previous].text.len()..lines[next].start;
        let existing = &text[range.clone()];
        let replacement =
            if is_stale_attached_gap(lines, previous, next, language_id, existing, line_ending) {
                line_ending.to_string()
            } else if needs_separator && !has_blank_separator(existing, line_ending) {
                line_ending.repeat(2)
            } else if !needs_separator
                && is_stale_c_allman_type_separator(
                    lines,
                    previous,
                    next,
                    language_id,
                    existing,
                    line_ending,
                )
            {
                line_ending.to_string()
            } else {
                existing.to_string()
            };
        if &text[range.clone()] != replacement {
            edits.push(TextEdit { range, replacement });
        }
    }

    let last = *significant_rows.last().unwrap();
    if (last + 1 < lines.len() || text.ends_with(line_ending))
        && row_is_active(last, &active_rows, scope)
    {
        let range = lines[last].start + lines[last].text.len()..text.len();
        let replacement = text
            .ends_with(line_ending)
            .then_some(line_ending)
            .unwrap_or_default();
        if &text[range.clone()] != replacement {
            edits.push(TextEdit {
                range,
                replacement: replacement.to_string(),
            });
        }
    }

    edits
}

fn has_blank_separator(text: &str, line_ending: &str) -> bool {
    text.matches(line_ending).count() >= 2
}

fn is_stale_c_allman_type_separator(
    lines: &[Line<'_>],
    previous: usize,
    next: usize,
    language_id: &str,
    existing: &str,
    line_ending: &str,
) -> bool {
    is_c_allman_language(language_id)
        && existing == line_ending.repeat(2)
        && c_like_initialized_declaration_type(lines[previous].text.trim())
            .zip(c_like_initialized_declaration_type(lines[next].text.trim()))
            .is_some_and(|(previous_type, next_type)| previous_type != next_type)
}

fn is_stale_attached_gap(
    lines: &[Line<'_>],
    previous: usize,
    next: usize,
    language_id: &str,
    existing: &str,
    line_ending: &str,
) -> bool {
    has_blank_separator(existing, line_ending)
        && (is_rust_attribute_item_pair(lines[previous].text, lines[next].text, language_id)
            || is_operator_led_expression_continuation(
                lines[previous].text,
                lines[next].text,
                language_id,
            ))
}

fn is_rust_attribute_item_pair(previous: &str, next: &str, language_id: &str) -> bool {
    language_id.eq_ignore_ascii_case("rust")
        && previous.trim().starts_with("#[")
        && is_rust_item_declaration(next.trim_start())
}

fn is_rust_item_declaration(text: &str) -> bool {
    let text = text
        .strip_prefix("pub ")
        .or_else(|| text.strip_prefix("pub(crate) "))
        .unwrap_or(text);
    [
        "struct ",
        "enum ",
        "union ",
        "trait ",
        "impl ",
        "fn ",
        "async fn ",
        "const ",
        "static ",
        "mod ",
        "type ",
        "use ",
        "extern ",
    ]
    .into_iter()
    .any(|prefix| text.starts_with(prefix))
}

fn is_operator_led_expression_continuation(previous: &str, next: &str, language_id: &str) -> bool {
    if !is_supported_brace_language(language_id) {
        return false;
    }
    let previous = previous.trim_end();
    if previous.is_empty()
        || previous.starts_with("//")
        || previous.starts_with("/*")
        || previous.ends_with([';', '{', '}'])
    {
        return false;
    }
    starts_with_expression_operator(next.trim_start())
}

fn is_supported_brace_language(language_id: &str) -> bool {
    matches!(
        language_id.to_ascii_lowercase().as_str(),
        "c" | "cpp" | "c++" | "c/c++" | "rust" | "go" | "javascript" | "typescript"
    )
}

fn starts_with_expression_operator(text: &str) -> bool {
    [
        "<<", ">>", "&&", "||", "->", "::", ".", "^", "|", "&", "+", "-", "*", "/", "%",
    ]
    .into_iter()
    .any(|operator| text.starts_with(operator))
}

fn structural_info(lines: &[Line<'_>], language_id: &str) -> StructuralInfo {
    if language_id.eq_ignore_ascii_case("python") {
        python_structural_info(lines)
    } else {
        c_like_structural_info(lines, language_id)
    }
}

fn structural_active_rows(length: usize, blocks: &[CodeBlock], scope: FormatScope) -> Vec<bool> {
    let mut active = vec![matches!(scope, FormatScope::Document); length];
    let FormatScope::Lines { start, end } = scope else {
        return active;
    };
    let selected_start = start.min(length.saturating_sub(1));
    let selected_end = end.min(length.saturating_sub(1));
    for row in selected_start..=selected_end {
        active[row] = true;
    }
    for block in blocks {
        if block.start <= selected_end && selected_start <= block.end {
            for row in block.start..=block.end.min(length.saturating_sub(1)) {
                active[row] = true;
            }
            if let Some(next) = active.get_mut(block.end.saturating_add(1)) {
                *next = true;
            }
        }
    }
    active
}

fn row_is_active(row: usize, active_rows: &[bool], scope: FormatScope) -> bool {
    matches!(scope, FormatScope::Document) || active_rows.get(row).copied().unwrap_or_default()
}

fn gap_is_active(previous: usize, next: usize, active_rows: &[bool], scope: FormatScope) -> bool {
    let _ = next;
    row_is_active(previous, active_rows, scope)
}

fn c_like_structural_info(lines: &[Line<'_>], language_id: &str) -> StructuralInfo {
    let mut info = StructuralInfo {
        blocks: Vec::new(),
        required_before: vec![false; lines.len()],
        import_rows: vec![false; lines.len()],
    };
    let mut scan_state = CLikeScanState::default();
    let mut stack = Vec::new();
    let mut previous_code = String::new();
    let mut go_import_group = false;

    for line in lines {
        let code = c_like_code(line.text, &mut scan_state, language_id);
        let trimmed = code.trim();
        let source_trimmed = line.text.trim();
        if language_id.eq_ignore_ascii_case("go") && source_trimmed.starts_with("import (") {
            go_import_group = true;
        }
        info.import_rows[line.row] = go_import_group || is_import_line(source_trimmed, language_id);
        if go_import_group && source_trimmed == ")" {
            go_import_group = false;
        }

        let bytes = code.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            match bytes[offset] {
                b'{' => {
                    let prefix = &code[..offset];
                    let do_while = contains_word(prefix, "do")
                        || (prefix.trim().is_empty() && contains_word(&previous_code, "do"));
                    stack.push((
                        is_code_brace(prefix, &previous_code, language_id),
                        line.row,
                        do_while,
                    ));
                    offset += 1;
                }
                b'}' => {
                    if let Some((is_code, start, do_while)) = stack.pop() {
                        if is_code {
                            info.blocks.push(CodeBlock {
                                start,
                                end: line.row,
                                do_while,
                            });
                        }
                    }
                    offset += 1;
                }
                _ => offset += char_len(&code, offset),
            }
        }
        if !trimmed.is_empty() {
            previous_code = trimmed.to_string();
        }
    }

    for block in &info.blocks {
        if closes_with_attached_c_like_continuation(lines[block.end].text, block.do_while) {
            continue;
        }
        let mut next = next_nonempty_row(lines, block.end);
        while let Some(row) = next {
            let text = lines[row].text.trim_start();
            if is_attached_c_like_continuation(text, block.do_while) {
                if block.do_while && text.starts_with("while") {
                    if let Some(after_while) = next_nonempty_row(lines, row) {
                        info.required_before[after_while] = true;
                    }
                }
                break;
            }
            if is_expression_continuation(text) {
                next = next_nonempty_row(lines, row);
                continue;
            }
            info.required_before[row] = true;
            break;
        }
    }

    for line in lines {
        if !is_declaration_header(line.text, language_id) {
            continue;
        }
        let Some(previous) = previous_nonempty_row(lines, line.row) else {
            continue;
        };
        let previous_text = lines[previous].text.trim();
        if previous_text.ends_with('{')
            || is_cpp_access_label(previous_text)
            || previous_text.starts_with("template ")
            || previous_text.starts_with("//")
            || previous_text.starts_with("/*")
            || (language_id.eq_ignore_ascii_case("rust") && previous_text.starts_with("#["))
            || (is_cpp_language(language_id)
                && previous_text.ends_with(',')
                && looks_like_cpp_initializer_row(line.text))
        {
            continue;
        }
        info.required_before[line.row] = true;
    }
    mark_c_allman_statement_boundaries(lines, language_id, &mut info.required_before);
    mark_c_allman_declaration_type_boundaries(lines, language_id, &mut info.required_before);
    mark_c_allman_aggregate_boundaries(lines, language_id, &mut info.required_before);
    info
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CAllmanStatement {
    Declaration(String),
    Mutation,
    Stream,
    Return,
    Other,
}

fn mark_c_allman_statement_boundaries(
    lines: &[Line<'_>],
    language_id: &str,
    required_before: &mut [bool],
) {
    if !is_c_allman_language(language_id) {
        return;
    }

    let mut previous = None::<(CAllmanStatement, usize)>;
    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        let statement = c_allman_statement(trimmed, language_id);
        let indent = indentation(line.text);
        if let Some((previous_statement, previous_indent)) = &previous {
            if *previous_indent == indent
                && c_allman_statement_needs_gap(previous_statement, &statement)
            {
                required_before[line.row] = true;
            }
        }
        previous = Some((statement, indent));
    }
}

fn mark_c_allman_declaration_type_boundaries(
    lines: &[Line<'_>],
    language_id: &str,
    required_before: &mut [bool],
) {
    if !is_c_allman_language(language_id) {
        return;
    }

    let mut declarations = Vec::<(usize, usize, String)>::new();
    for line in lines {
        let trimmed = line.text.trim();
        let declaration =
            (!trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with("/*"))
                .then(|| c_like_initialized_declaration_type(trimmed))
                .flatten();
        let indent = indentation(line.text);
        match declaration {
            Some(type_name)
                if declarations
                    .last()
                    .is_none_or(|(_, previous_indent, _)| *previous_indent == indent) =>
            {
                declarations.push((line.row, indent, type_name));
            }
            Some(type_name) => {
                mark_declaration_run_type_boundaries(&declarations, required_before);
                declarations.clear();
                declarations.push((line.row, indent, type_name));
            }
            None => {
                mark_declaration_run_type_boundaries(&declarations, required_before);
                declarations.clear();
            }
        }
    }
    mark_declaration_run_type_boundaries(&declarations, required_before);
}

fn mark_declaration_run_type_boundaries(
    declarations: &[(usize, usize, String)],
    required_before: &mut [bool],
) {
    let mut run_start = 0;
    while run_start < declarations.len() {
        let type_name = &declarations[run_start].2;
        let run_end = declarations[run_start..]
            .iter()
            .take_while(|(_, _, candidate)| candidate == type_name)
            .count()
            + run_start;
        if run_end < declarations.len() {
            let next_type = &declarations[run_end].2;
            let next_end = declarations[run_end..]
                .iter()
                .take_while(|(_, _, candidate)| candidate == next_type)
                .count()
                + run_end;
            if run_end - run_start >= 3 && next_end - run_end >= 3 {
                required_before[declarations[run_end].0] = true;
            }
        }
        run_start = run_end;
    }
}

fn mark_c_allman_aggregate_boundaries(
    lines: &[Line<'_>],
    language_id: &str,
    required_before: &mut [bool],
) {
    if !is_c_allman_language(language_id) {
        return;
    }

    for (index, line) in lines.iter().enumerate() {
        if is_designated_aggregate_header(lines, index) {
            if let Some(previous) = lines[..index]
                .iter()
                .rev()
                .find(|candidate| !candidate.text.trim().is_empty())
            {
                if previous.text.trim() != "{" {
                    required_before[line.row] = true;
                }
            }
        }
        if line.text.trim() == "};" {
            if let Some(next) = lines[index + 1..]
                .iter()
                .find(|candidate| !candidate.text.trim().is_empty())
            {
                required_before[next.row] = true;
            }
        }
    }
}

fn is_designated_aggregate_header(lines: &[Line<'_>], index: usize) -> bool {
    let header = lines[index]
        .text
        .trim()
        .strip_suffix('{')
        .map(str::trim_end);
    let Some(header) = header else {
        return false;
    };
    if header.is_empty() || is_c_like_control_header(header) {
        return false;
    }
    lines[index + 1..]
        .iter()
        .find(|line| !line.text.trim().is_empty())
        .is_some_and(|line| line.text.trim_start().starts_with('.'))
}

fn c_allman_statement(line: &str, language_id: &str) -> CAllmanStatement {
    if let Some(type_name) = c_like_initialized_declaration_type(line) {
        return CAllmanStatement::Declaration(type_name);
    }
    if is_cpp_language(language_id) && (line.starts_with("std::cout") || line.starts_with("<<")) {
        return CAllmanStatement::Stream;
    }
    if line.starts_with("return ") || line == "return;" {
        return CAllmanStatement::Return;
    }
    find_separator(line)
        .filter(|separator| matches!(separator.kind, "=" | "+=" | "-=" | "*=" | "/=" | "%="))
        .map(|_| CAllmanStatement::Mutation)
        .unwrap_or(CAllmanStatement::Other)
}

fn c_allman_statement_needs_gap(previous: &CAllmanStatement, next: &CAllmanStatement) -> bool {
    match (previous, next) {
        (CAllmanStatement::Declaration(_), CAllmanStatement::Declaration(_)) => false,
        (
            CAllmanStatement::Declaration(_),
            CAllmanStatement::Mutation | CAllmanStatement::Stream | CAllmanStatement::Return,
        )
        | (
            CAllmanStatement::Mutation,
            CAllmanStatement::Declaration(_) | CAllmanStatement::Stream | CAllmanStatement::Return,
        )
        | (
            CAllmanStatement::Stream,
            CAllmanStatement::Declaration(_)
            | CAllmanStatement::Mutation
            | CAllmanStatement::Return,
        )
        | (
            CAllmanStatement::Return,
            CAllmanStatement::Declaration(_)
            | CAllmanStatement::Mutation
            | CAllmanStatement::Stream,
        ) => true,
        _ => false,
    }
}

fn closes_with_attached_c_like_continuation(line: &str, do_while: bool) -> bool {
    line.find('}').is_some_and(|close| {
        is_attached_c_like_continuation(line[close + 1..].trim_start(), do_while)
    })
}

#[derive(Default)]
struct CLikeScanState {
    block_comment: bool,
}

fn c_like_code(line: &str, state: &mut CLikeScanState, language_id: &str) -> String {
    let bytes = line.as_bytes();
    let mut result = vec![b' '; bytes.len()];
    let mut offset = 0;
    let mut quote = None;
    let mut escaped = false;

    while offset < bytes.len() {
        if state.block_comment {
            if bytes[offset..].starts_with(b"*/") {
                state.block_comment = false;
                offset += 2;
            } else {
                offset += char_len(line, offset);
            }
            continue;
        }
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == active_quote {
                quote = None;
            }
            offset += char_len(line, offset);
            continue;
        }
        if bytes[offset..].starts_with(b"//") {
            break;
        }
        if bytes[offset..].starts_with(b"/*") {
            state.block_comment = true;
            offset += 2;
            continue;
        }
        if language_id.eq_ignore_ascii_case("rust")
            && bytes[offset] == b'\''
            && is_rust_lifetime_start(line, offset)
        {
            result[offset] = bytes[offset];
            offset += 1;
            continue;
        }
        if matches!(bytes[offset], b'\'' | b'"' | b'`') {
            quote = Some(bytes[offset]);
            offset += 1;
            continue;
        }
        let width = char_len(line, offset);
        result[offset..offset + width].copy_from_slice(&bytes[offset..offset + width]);
        offset += width;
    }
    String::from_utf8(result).expect("source text remains valid UTF-8")
}

fn is_code_brace(prefix: &str, previous_code: &str, _language_id: &str) -> bool {
    let context = if prefix.trim().is_empty() {
        previous_code.trim_end()
    } else {
        prefix.trim_end()
    };
    let lower = context.to_ascii_lowercase();
    if [
        "if",
        "else",
        "for",
        "while",
        "switch",
        "case",
        "catch",
        "try",
        "do",
        "class",
        "struct",
        "union",
        "enum",
        "namespace",
        "impl",
        "trait",
        "match",
        "function",
        "func",
        "fn",
    ]
    .into_iter()
    .any(|word| contains_word(&lower, word))
        || lower.contains("=>")
    {
        return true;
    }
    if context.ends_with(')') || context.ends_with(']') {
        return true;
    }
    if context.contains('(')
        && ["const", "noexcept", "override", "final"]
            .into_iter()
            .any(|suffix| context.ends_with(suffix))
    {
        return true;
    }
    if context
        .chars()
        .last()
        .is_some_and(|character| matches!(character, '=' | '(' | ',' | ':' | '['))
    {
        return false;
    }
    if context
        .chars()
        .last()
        .is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '>'))
    {
        return false;
    }
    true
}

fn contains_word(text: &str, word: &str) -> bool {
    text.split(|character: char| !character.is_alphanumeric() && character != '_')
        .any(|candidate| candidate == word)
}

fn next_nonempty_row(lines: &[Line<'_>], row: usize) -> Option<usize> {
    lines
        .iter()
        .skip(row.saturating_add(1))
        .find(|line| !line.text.trim().is_empty())
        .map(|line| line.row)
}

fn previous_nonempty_row(lines: &[Line<'_>], row: usize) -> Option<usize> {
    lines
        .iter()
        .take(row)
        .rev()
        .find(|line| !line.text.trim().is_empty())
        .map(|line| line.row)
}

fn is_declaration_header(line: &str, language_id: &str) -> bool {
    let text = line.trim_start();
    if text.starts_with('[') || starts_with_expression_operator(text) {
        return false;
    }
    if text.starts_with("class ")
        || text.starts_with("struct ")
        || text.starts_with("union ")
        || text.starts_with("enum ")
        || text.starts_with("trait ")
        || text.starts_with("impl ")
        || text.starts_with("def ")
        || text.starts_with("fn ")
        || text.starts_with("func ")
        || text.starts_with("function ")
    {
        return true;
    }
    if language_id.eq_ignore_ascii_case("python") {
        return false;
    }
    let Some(open) = first_open_parenthesis(text) else {
        return false;
    };
    let prefix = text[..open].trim_start();
    if is_c_like_control_header(prefix) || prefix.contains(['.', '=']) {
        return false;
    }
    let Some(close) = matching_parenthesis(text, open) else {
        return prefix.split_whitespace().count() >= 2;
    };
    let suffix = text[close + 1..].trim_start();
    suffix.starts_with('{')
        || suffix.starts_with(':')
        || suffix.starts_with("->")
        || (prefix.split_whitespace().count() >= 2 && suffix.is_empty())
}

fn looks_like_cpp_initializer_row(line: &str) -> bool {
    let text = line.trim_start();
    let Some(open) = first_open_parenthesis(text) else {
        return false;
    };
    let prefix = text[..open].trim();
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

fn is_c_like_control_header(text: &str) -> bool {
    ["if", "for", "while", "switch", "catch", "return", "sizeof"]
        .into_iter()
        .any(|keyword| {
            text == keyword
                || text.strip_prefix(keyword).is_some_and(|suffix| {
                    suffix.starts_with(|character: char| {
                        character.is_whitespace() || character == '('
                    })
                })
        })
}

fn is_attached_c_like_continuation(text: &str, do_while: bool) -> bool {
    text.starts_with("else")
        || text.starts_with("catch")
        || text.starts_with("finally")
        || (do_while && text.starts_with("while"))
}

fn is_expression_continuation(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|character| matches!(character, '}' | ')' | ']' | ',' | ';' | '.'))
        || text.starts_with("->")
        || text.starts_with("::")
}

fn is_import_line(text: &str, language_id: &str) -> bool {
    if is_c_like_language(language_id) {
        text.starts_with("#include") || text.starts_with("#import")
    } else if language_id.eq_ignore_ascii_case("rust") {
        text.starts_with("use ") || text.starts_with("extern crate ")
    } else if language_id.eq_ignore_ascii_case("go") {
        text.starts_with("import ")
    } else {
        text.starts_with("import ")
    }
}

fn python_structural_info(lines: &[Line<'_>]) -> StructuralInfo {
    let mut info = StructuralInfo {
        blocks: Vec::new(),
        required_before: vec![false; lines.len()],
        import_rows: vec![false; lines.len()],
    };
    let mut stack = Vec::new();
    let mut previous_content = None;

    for line in lines {
        let code = python_code(line.text);
        let trimmed = code.trim();
        info.import_rows[line.row] = trimmed.starts_with("import ") || trimmed.starts_with("from ");
        if trimmed.is_empty() {
            continue;
        }
        let indent_end = indentation(line.text);
        let indent = logical_column(&line.text[..indent_end], 4);
        let attached = is_python_continuation(trimmed);
        let mut closed = false;
        while stack
            .last()
            .is_some_and(|(block_indent, _)| indent <= *block_indent)
        {
            let (block_indent, start) = stack.pop().unwrap();
            let end = previous_content.unwrap_or(line.row).max(start);
            info.blocks.push(CodeBlock {
                start,
                end,
                do_while: false,
            });
            debug_assert!(block_indent >= indent);
            closed = true;
        }
        if closed && !attached {
            info.required_before[line.row] = true;
        }
        if is_python_suite(trimmed) {
            stack.push((indent, line.row));
        }
        previous_content = Some(line.row);
    }
    info
}

fn python_code(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut result = vec![b' '; bytes.len()];
    let mut offset = 0;
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == active_quote {
                quote = None;
            }
            offset += char_len(line, offset);
            continue;
        }
        if bytes[offset] == b'#' {
            break;
        }
        if matches!(bytes[offset], b'\'' | b'"') {
            quote = Some(bytes[offset]);
            offset += 1;
            continue;
        }
        let width = char_len(line, offset);
        result[offset..offset + width].copy_from_slice(&bytes[offset..offset + width]);
        offset += width;
    }
    String::from_utf8(result).expect("source text remains valid UTF-8")
}

fn is_python_suite(text: &str) -> bool {
    text.ends_with(':')
        && [
            "if ",
            "elif ",
            "else:",
            "for ",
            "while ",
            "try:",
            "except",
            "finally:",
            "def ",
            "class ",
            "with ",
            "match ",
            "case ",
            "async def ",
            "async for ",
            "async with ",
        ]
        .into_iter()
        .any(|prefix| text.starts_with(prefix))
}

fn is_python_continuation(text: &str) -> bool {
    ["elif ", "else:", "except", "finally:"]
        .into_iter()
        .any(|prefix| text.starts_with(prefix))
}

fn multiline_excluded_rows(lines: &[Line<'_>], language_id: &str) -> Vec<bool> {
    let mut state = MultilineState::default();
    lines
        .iter()
        .map(|line| line_contains_multiline_literal_or_comment(line.text, &mut state, language_id))
        .collect()
}

fn line_contains_multiline_literal_or_comment(
    line: &str,
    state: &mut MultilineState,
    language_id: &str,
) -> bool {
    let bytes = line.as_bytes();
    let mut offset = 0;
    let mut excluded = state.block_comment || state.quote.is_some() || state.triple_quote.is_some();
    let mut escaped = false;

    while offset < bytes.len() {
        if state.block_comment {
            excluded = true;
            if bytes[offset..].starts_with(b"*/") {
                state.block_comment = false;
                offset += 2;
            } else {
                offset += char_len(line, offset);
            }
            continue;
        }

        if let Some(quote) = state.triple_quote {
            excluded = true;
            if bytes[offset..].starts_with(&[quote, quote, quote]) {
                state.triple_quote = None;
                offset += 3;
            } else {
                offset += char_len(line, offset);
            }
            continue;
        }

        if let Some(quote) = state.quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == quote {
                state.quote = None;
            }
            offset += char_len(line, offset);
            continue;
        }

        if bytes[offset..].starts_with(b"/*") {
            state.block_comment = true;
            excluded = true;
            offset += 2;
        } else if matches!(bytes[offset], b'\'' | b'"')
            && bytes[offset..].starts_with(&[bytes[offset], bytes[offset], bytes[offset]])
        {
            state.triple_quote = Some(bytes[offset]);
            excluded = true;
            offset += 3;
        } else if language_id.eq_ignore_ascii_case("rust")
            && bytes[offset] == b'\''
            && is_rust_lifetime_start(line, offset)
        {
            offset += 1;
        } else if matches!(bytes[offset], b'\'' | b'"' | b'`') {
            state.quote = Some(bytes[offset]);
            offset += 1;
        } else {
            offset += char_len(line, offset);
        }
    }

    excluded || state.block_comment || state.quote.is_some() || state.triple_quote.is_some()
}

fn is_rust_lifetime_start(line: &str, quote_offset: usize) -> bool {
    let lifetime = &line[quote_offset + 1..];
    let mut end = 0;
    for character in lifetime.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            end += character.len_utf8();
        } else {
            break;
        }
    }
    end > 0
        && lifetime[end..].chars().next().is_none_or(|character| {
            character.is_whitespace()
                || matches!(
                    character,
                    '>' | ',' | ')' | ':' | '=' | '+' | '-' | '*' | '/' | '&' | '|'
                )
        })
}

fn contiguous_groups(candidates: &[Option<Candidate>]) -> Vec<Vec<Candidate>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();

    for candidate in candidates.iter().flatten() {
        let continues = current.last().is_some_and(|previous: &Candidate| {
            previous.row + 1 == candidate.row
                && previous.indent == candidate.indent
                && previous.separator.kind == candidate.separator.kind
        });
        if !continues && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(candidate.clone());
    }

    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn alignment_edits_for_group(
    lines: &[Line<'_>],
    group: &[Candidate],
    tab_size: usize,
) -> Vec<TextEdit> {
    let target_column = group
        .iter()
        .map(|candidate| {
            let line = lines[candidate.row].text;
            alignment_target_column(line, &candidate.separator, tab_size)
        })
        .max()
        .unwrap_or_default();

    let mut edits = Vec::new();
    for candidate in group {
        let line = lines[candidate.row];
        let before_separator = &line.text[..candidate.separator.whitespace_start];
        let column = logical_column(before_separator, tab_size);
        let replacement = " ".repeat(target_column.saturating_sub(column));
        let range = line.start + candidate.separator.whitespace_start
            ..line.start + candidate.separator.range.start;
        if line.text[candidate.separator.whitespace_start..candidate.separator.range.start]
            != replacement
        {
            edits.push(TextEdit { range, replacement });
        }
        if let Some(range) = &candidate.separator.trailing_whitespace_before_terminator {
            if !range.is_empty() {
                edits.push(TextEdit {
                    range: line.start + range.start..line.start + range.end,
                    replacement: String::new(),
                });
            }
        }
    }
    edits
}

fn alignment_target_column(line: &str, separator: &Separator, tab_size: usize) -> usize {
    if separator.kind == "cpp_initializer" {
        logical_column(&line[..separator.range.start], tab_size)
    } else {
        logical_column(&line[..separator.whitespace_start], tab_size) + 1
    }
}

fn print_label_alignment_edits(
    lines: &[Line<'_>],
    excluded_rows: &[bool],
    language_id: &str,
    tab_size: usize,
    scope: FormatScope,
) -> Vec<TextEdit> {
    let mut candidates = Vec::with_capacity(lines.len());
    let mut cpp_stream_indent = None;

    for (line, excluded) in lines.iter().zip(excluded_rows) {
        let trimmed = line.text.trim_start();
        let cpp_stream_head = is_cpp_language(language_id) && is_cpp_standard_stream_head(trimmed);
        if cpp_stream_head {
            cpp_stream_indent = Some(logical_column(
                &line.text[..indentation(line.text)],
                tab_size,
            ));
        }
        let cpp_stream_continuation = cpp_stream_indent.is_some() && trimmed.starts_with("<<");
        let is_cpp_stream_row = cpp_stream_head || cpp_stream_continuation;

        let candidate = (!excluded)
            .then(|| {
                find_print_label_candidate(
                    line,
                    language_id,
                    tab_size,
                    is_cpp_stream_row,
                    cpp_stream_indent,
                )
            })
            .flatten();
        candidates.push(candidate);

        if !is_cpp_stream_row || contains_unquoted_semicolon(line.text) {
            cpp_stream_indent = None;
        }
    }

    let mut edits = Vec::new();
    for group in contiguous_print_label_groups(&candidates, lines) {
        if group.len() < 2 || !print_label_group_intersects_scope(&group, scope) {
            continue;
        }

        let target_width = group
            .iter()
            .map(|candidate| candidate.label_width + 1)
            .max()
            .unwrap_or_default();
        for candidate in group {
            let line = lines[candidate.row];
            let replacement = " ".repeat(target_width.saturating_sub(candidate.label_width));
            let range = line.start + candidate.padding_range.start
                ..line.start + candidate.padding_range.end;
            if line.text[candidate.padding_range] != replacement {
                edits.push(TextEdit { range, replacement });
            }
        }
    }
    edits
}

fn contiguous_print_label_groups(
    candidates: &[Option<PrintLabelCandidate>],
    lines: &[Line<'_>],
) -> Vec<Vec<PrintLabelCandidate>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();

    for candidate in candidates.iter().flatten() {
        let continues = current
            .last()
            .is_some_and(|previous: &PrintLabelCandidate| {
                print_label_rows_are_contiguous(previous, candidate, lines)
                    && previous.indent == candidate.indent
                    && previous.delimiter == candidate.delimiter
            });
        if !continues && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(candidate.clone());
    }

    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn print_label_rows_are_contiguous(
    previous: &PrintLabelCandidate,
    candidate: &PrintLabelCandidate,
    lines: &[Line<'_>],
) -> bool {
    previous.row + 1 == candidate.row
        || (previous.row + 2 == candidate.row
            && lines
                .get(previous.row + 1)
                .is_some_and(|line| is_cpp_standard_stream_head(line.text.trim_start())))
}

fn print_label_group_intersects_scope(group: &[PrintLabelCandidate], scope: FormatScope) -> bool {
    match scope {
        FormatScope::Document => true,
        FormatScope::Lines { start, end } => group
            .iter()
            .any(|candidate| (start..=end).contains(&candidate.row)),
    }
}

fn find_print_label_candidate(
    line: &Line<'_>,
    language_id: &str,
    tab_size: usize,
    cpp_stream_row: bool,
    cpp_stream_indent: Option<usize>,
) -> Option<PrintLabelCandidate> {
    let literal = find_output_literal(line.text, language_id, cpp_stream_row)?;
    let content = &line.text[literal.content_start..literal.content_end];
    let (delimiter_offset, delimiter) = find_print_label_delimiter(content)?;
    let label = &content[..delimiter_offset];
    let label_end = label.trim_end_matches(' ').len();
    let label = &label[..label_end];
    if !is_safe_print_label(label) || !has_safe_print_label_tail(&content[delimiter_offset + 1..]) {
        return None;
    }

    Some(PrintLabelCandidate {
        row: line.row,
        indent: cpp_stream_indent
            .unwrap_or_else(|| logical_column(&line.text[..indentation(line.text)], tab_size)),
        delimiter,
        padding_range: literal.content_start + label_end..literal.content_start + delimiter_offset,
        label_width: logical_column(label, tab_size),
    })
}

#[derive(Debug, Clone, Copy)]
struct OutputLiteral {
    content_start: usize,
    content_end: usize,
}

fn find_output_literal(
    line: &str,
    language_id: &str,
    cpp_stream_row: bool,
) -> Option<OutputLiteral> {
    let trimmed_start = indentation(line);
    let trimmed = &line[trimmed_start..];

    if cpp_stream_row {
        let insertion = find_unquoted_substring(trimmed, "<<")?;
        return find_static_output_literal(
            line,
            trimmed_start + insertion + 2,
            false,
            OutputLiteralQuotes::Double,
        );
    }

    let (open, allows_destination, quotes) = match language_id.to_ascii_lowercase().as_str() {
        "rust" => (
            call_open_after_name(trimmed, &["print!", "println!", "eprint!", "eprintln!"])?,
            false,
            OutputLiteralQuotes::Double,
        ),
        "c" => {
            let direct = call_open_after_name(trimmed, &["printf"])
                .map(|open| (open, false))
                .or_else(|| call_open_after_name(trimmed, &["fprintf"]).map(|open| (open, true)));
            let (open, allows_destination) = direct?;
            (open, allows_destination, OutputLiteralQuotes::Double)
        }
        "cpp" | "c++" | "c/c++" => {
            let direct = call_open_after_name(trimmed, &["std::print", "std::println", "printf"])
                .map(|open| (open, false));
            let writer = call_open_after_name(trimmed, &["fprintf"]).map(|open| (open, true));
            let (open, allows_destination) = direct.or(writer)?;
            (open, allows_destination, OutputLiteralQuotes::Double)
        }
        "go" => {
            let direct = call_open_after_name(
                trimmed,
                &[
                    "fmt.Print",
                    "fmt.Printf",
                    "fmt.Println",
                    "log.Print",
                    "log.Printf",
                    "log.Println",
                    "log.Fatal",
                    "log.Fatalf",
                    "log.Fatalln",
                    "log.Panic",
                    "log.Panicf",
                    "log.Panicln",
                ],
            )
            .map(|open| (open, false));
            let writer =
                call_open_after_name(trimmed, &["fmt.Fprint", "fmt.Fprintf", "fmt.Fprintln"])
                    .map(|open| (open, true));
            let (open, allows_destination) = direct.or(writer)?;
            (open, allows_destination, OutputLiteralQuotes::Double)
        }
        "python" | "py" => {
            let direct = call_open_after_name(trimmed, &["print"]).map(|open| (open, false));
            let logging = call_open_after_name(
                trimmed,
                &[
                    "logging.debug",
                    "logging.info",
                    "logging.warning",
                    "logging.error",
                    "logging.critical",
                    "logging.exception",
                ],
            )
            .map(|open| (open, false));
            let (open, allows_destination) = direct.or(logging)?;
            (
                open,
                allows_destination,
                OutputLiteralQuotes::SingleOrDouble,
            )
        }
        "javascript" | "js" | "typescript" | "ts" | "tsx" | "jsx" => {
            let direct = call_open_after_name(
                trimmed,
                &[
                    "console.log",
                    "console.info",
                    "console.warn",
                    "console.error",
                    "console.debug",
                    "process.stdout.write",
                    "process.stderr.write",
                ],
            )?;
            (direct, false, OutputLiteralQuotes::JavaScript)
        }
        _ => return None,
    };

    find_static_output_literal(line, trimmed_start + open, allows_destination, quotes)
}

#[derive(Debug, Clone, Copy)]
enum OutputLiteralQuotes {
    Double,
    SingleOrDouble,
    JavaScript,
}

fn call_open_after_name(text: &str, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|name| {
        let suffix = text.strip_prefix(name)?;
        let whitespace = suffix.len() - suffix.trim_start().len();
        suffix[whitespace..]
            .strip_prefix('(')
            .map(|_| name.len() + whitespace + 1)
    })
}

fn find_static_output_literal(
    line: &str,
    argument_start: usize,
    allows_destination: bool,
    quotes: OutputLiteralQuotes,
) -> Option<OutputLiteral> {
    let bytes = line.as_bytes();
    let mut offset = argument_start;
    while matches!(bytes.get(offset), Some(b' ' | b'\t')) {
        offset += 1;
    }

    let string_prefix_end = offset;
    if matches!(quotes, OutputLiteralQuotes::SingleOrDouble)
        && matches!(bytes.get(offset), Some(b'f' | b'F'))
    {
        offset += 1;
    }

    if !bytes
        .get(offset)
        .is_some_and(|quote| output_literal_quote_is_allowed(*quote, quotes))
    {
        offset = find_next_output_literal_quote(line, offset, quotes)?;
    }

    let quote = *bytes.get(offset)?;
    if bytes.get(offset + 1) == Some(&quote) {
        return None;
    }

    let prefix = &line[argument_start..string_prefix_end];
    if !(prefix.trim().is_empty()
        || (allows_destination && is_simple_output_destination(prefix.trim())))
    {
        return None;
    }

    let content_start = offset + 1;
    let content_end = closing_quote(line, content_start, quote)?;
    Some(OutputLiteral {
        content_start,
        content_end,
    })
}

fn find_next_output_literal_quote(
    line: &str,
    start: usize,
    quotes: OutputLiteralQuotes,
) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut offset = start;
    while offset < bytes.len() && !matches!(bytes[offset], b')' | b'\n' | b'\r') {
        if output_literal_quote_is_allowed(bytes[offset], quotes) {
            return Some(offset);
        }
        offset += char_len(line, offset);
    }
    None
}

fn output_literal_quote_is_allowed(quote: u8, quotes: OutputLiteralQuotes) -> bool {
    match quotes {
        OutputLiteralQuotes::Double => quote == b'"',
        OutputLiteralQuotes::SingleOrDouble => matches!(quote, b'\'' | b'"'),
        OutputLiteralQuotes::JavaScript => matches!(quote, b'\'' | b'"' | b'`'),
    }
}

fn is_simple_output_destination(prefix: &str) -> bool {
    let Some((destination, trailing)) = prefix.rsplit_once(',') else {
        return false;
    };
    trailing.trim().is_empty()
        && destination
            .trim()
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
}

fn closing_quote(line: &str, content_start: usize, quote: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut offset = content_start;
    let mut escaped = false;
    while offset < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[offset] == b'\\' {
            escaped = true;
        } else if bytes[offset] == quote {
            return Some(offset);
        }
        offset += char_len(line, offset);
    }
    None
}

fn find_print_label_delimiter(content: &str) -> Option<(usize, u8)> {
    let bytes = content.as_bytes();
    let mut escaped = false;
    for (offset, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
        } else if matches!(byte, b':' | b'=' | b'|') {
            return Some((offset, byte));
        }
    }
    None
}

fn is_safe_print_label(label: &str) -> bool {
    !label.is_empty()
        && label.is_ascii()
        && label.bytes().any(|byte| byte.is_ascii_alphanumeric())
        && label.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')' | b'[' | b']')
        })
}

fn has_safe_print_label_tail(tail: &str) -> bool {
    let tail = tail.trim_start_matches([' ', '\t']);
    tail.is_empty()
        || tail.starts_with('{')
        || tail.starts_with('%')
        || tail.starts_with("${")
        || tail.starts_with("\\n")
        || tail.starts_with("\\r")
        || tail.starts_with("\\t")
}

fn is_cpp_standard_stream_head(text: &str) -> bool {
    ["std::cout", "std::cerr", "std::clog"]
        .into_iter()
        .any(|stream| {
            text.strip_prefix(stream).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(|character: char| {
                        character.is_whitespace() || character == '<'
                    })
            })
        })
}

fn find_unquoted_substring(text: &str, needle: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == active_quote {
                quote = None;
            }
            offset += char_len(text, offset);
            continue;
        }
        if matches!(bytes[offset], b'\'' | b'"' | b'`') {
            quote = Some(bytes[offset]);
            offset += 1;
        } else if text[offset..].starts_with(needle) {
            return Some(offset);
        } else {
            offset += char_len(text, offset);
        }
    }
    None
}

fn contains_unquoted_semicolon(text: &str) -> bool {
    find_unquoted_substring(text, ";").is_some()
}

fn find_alignment_separator(line: &str, language_id: &str) -> Option<Separator> {
    find_separator_for_language(line, language_id)
        .or_else(|| {
            is_c_like_language(language_id)
                .then(|| find_c_like_declaration(line))
                .flatten()
        })
        .or_else(|| {
            is_c_like_language(language_id)
                .then(|| find_c_like_bitfield(line))
                .flatten()
        })
        .or_else(|| {
            language_id
                .eq_ignore_ascii_case("go")
                .then(|| find_go_field(line))
                .flatten()
        })
        .or_else(|| {
            is_cpp_language(language_id)
                .then(|| find_cpp_initializer(line))
                .flatten()
        })
        .or_else(|| {
            is_cpp_language(language_id)
                .then(|| find_cpp_stream_insertion(line))
                .flatten()
        })
}

fn find_separator(line: &str) -> Option<Separator> {
    find_separator_with_rust_lifetimes(line, false)
}

fn find_separator_for_language(line: &str, language_id: &str) -> Option<Separator> {
    find_separator_with_rust_lifetimes(line, language_id.eq_ignore_ascii_case("rust"))
}

fn find_separator_with_rust_lifetimes(
    line: &str,
    recognizes_rust_lifetimes: bool,
) -> Option<Separator> {
    let mut offset = 0;
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut escaped = false;

    while offset < bytes.len() {
        let byte = bytes[offset];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            offset += char_len(line, offset);
            continue;
        }

        if recognizes_rust_lifetimes && byte == b'\'' && is_rust_lifetime_start(line, offset) {
            offset += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
            offset += 1;
            continue;
        }
        if byte == b'#' || bytes[offset..].starts_with(b"//") || bytes[offset..].starts_with(b"/*")
        {
            return None;
        }

        let (kind, width) = if bytes[offset..].starts_with(b"=>") {
            ("=>", 2)
        } else if bytes[offset..].starts_with(b":=") {
            (":=", 2)
        } else if matches!(byte, b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|')
            && bytes.get(offset + 1) == Some(&b'=')
        {
            (
                match byte {
                    b'+' => "+=",
                    b'-' => "-=",
                    b'*' => "*=",
                    b'/' => "/=",
                    b'%' => "%=",
                    b'&' => "&=",
                    _ => "|=",
                },
                2,
            )
        } else if byte == b'=' {
            if matches!(bytes.get(offset + 1), Some(b'=' | b'>'))
                || matches!(
                    offset.checked_sub(1).and_then(|index| bytes.get(index)),
                    Some(b'!' | b'<' | b'>' | b'=')
                )
            {
                offset += 1;
                continue;
            }
            ("=", 1)
        } else if byte == b':' {
            if matches!(bytes.get(offset + 1), Some(b':' | b'='))
                || matches!(
                    offset.checked_sub(1).and_then(|index| bytes.get(index)),
                    Some(b':' | b'?')
                )
            {
                offset += 1;
                continue;
            }
            (":", 1)
        } else {
            offset += char_len(line, offset);
            continue;
        };

        let whitespace_start = line[..offset]
            .char_indices()
            .rev()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(0, |(index, character)| index + character.len_utf8());
        let separator = Separator {
            kind,
            range: offset..offset + width,
            whitespace_start,
            trailing_whitespace_before_terminator: None,
        };
        if is_safe_separator(line, &separator) {
            return Some(separator);
        }
        offset += width;
    }
    None
}

fn is_safe_separator(line: &str, separator: &Separator) -> bool {
    let lhs = line[..separator.whitespace_start].trim();
    let rhs = line[separator.range.end..].trim();
    if lhs.is_empty() || rhs.is_empty() || lhs.contains([';', '{', '}']) {
        return false;
    }

    match separator.kind {
        ":" | "=>" => is_field_lhs(lhs),
        _ => lhs
            .chars()
            .any(|character| character.is_alphabetic() || character == '_'),
    }
}

fn is_field_lhs(lhs: &str) -> bool {
    let lhs = lhs.trim_start_matches("...").trim_end_matches('?');
    (lhs.starts_with('"') && lhs.ends_with('"'))
        || (lhs.starts_with('\'') && lhs.ends_with('\''))
        || lhs
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '.' | '-'))
}

fn is_c_like_language(language_id: &str) -> bool {
    matches!(
        language_id.to_ascii_lowercase().as_str(),
        "c" | "cpp" | "c++" | "c/c++"
    )
}

fn is_c_allman_language(language_id: &str) -> bool {
    matches!(
        language_id.to_ascii_lowercase().as_str(),
        "c" | "cpp" | "c++" | "c/c++"
    )
}

fn is_cpp_language(language_id: &str) -> bool {
    matches!(
        language_id.to_ascii_lowercase().as_str(),
        "cpp" | "c++" | "c/c++"
    )
}

fn c_like_initialized_declaration_type(line: &str) -> Option<String> {
    let separator = find_separator(line)?;
    if separator.kind != "="
        || line.trim_start().starts_with('.')
        || !line.trim_end().ends_with(';')
    {
        return None;
    }
    let lhs = line[..separator.whitespace_start].trim_end();
    if lhs.contains(['(', ')', '[', ']', '.', '>']) {
        return None;
    }
    let name_start = last_identifier_start(lhs)?;
    let before_name = &lhs[..name_start];
    let type_prefix = before_name.trim_end();
    if type_prefix.is_empty() || is_c_like_statement(type_prefix) {
        return None;
    }
    let separator_before_name = before_name.chars().last().is_some_and(char::is_whitespace)
        || type_prefix
            .chars()
            .last()
            .is_some_and(|character| matches!(character, '*' | '&'));
    separator_before_name.then(|| type_prefix.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn find_cpp_initializer(line: &str) -> Option<Separator> {
    let comment_start = line.find("//").unwrap_or(line.len());
    let code = &line[..comment_start];
    let code_end = code.trim_end().len();
    let code = &code[..code_end];
    let content_start = code.len() - code.trim_start().len();
    let mut member_start = content_start;
    let mut whitespace_start = content_start;

    if code[content_start..].starts_with(':') {
        member_start += 1;
        whitespace_start = member_start;
        member_start += code[member_start..].len() - code[member_start..].trim_start().len();
    }

    let identifier_end = identifier_end(code, member_start)?;
    let member = &code[member_start..identifier_end];
    if matches!(member, "if" | "for" | "while" | "switch" | "catch") {
        return None;
    }
    let open =
        identifier_end + code[identifier_end..].len() - code[identifier_end..].trim_start().len();
    if code.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let close = matching_parenthesis(code, open)?;
    let tail = code[close + 1..].trim();
    if !tail.is_empty() && tail != "," {
        return None;
    }

    Some(Separator {
        kind: "cpp_initializer",
        range: member_start..member_start,
        whitespace_start,
        trailing_whitespace_before_terminator: None,
    })
}

fn find_cpp_stream_insertion(line: &str) -> Option<Separator> {
    let comment_start = line.find("//").unwrap_or(line.len());
    let code = &line[..comment_start];
    let content_start = code.len() - code.trim_start().len();
    if !code[content_start..].starts_with("<<") {
        return None;
    }

    let mut offsets = Vec::new();
    let mut offset = content_start;
    let bytes = code.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            offset += char_len(code, offset);
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            offset += 1;
        } else if bytes[offset..].starts_with(b"<<") {
            offsets.push(offset);
            offset += 2;
        } else {
            offset += char_len(code, offset);
        }
    }
    let separator_start = *offsets.get(1..)?.last()?;
    let whitespace_start = code[..separator_start]
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_whitespace())
        .map_or(0, |(index, character)| index + character.len_utf8());
    Some(Separator {
        kind: "cpp_stream_insertion",
        range: separator_start..separator_start + 2,
        whitespace_start,
        trailing_whitespace_before_terminator: None,
    })
}

fn identifier_end(text: &str, start: usize) -> Option<usize> {
    let mut end = start;
    let mut characters = text[start..].char_indices();
    let (_, first) = characters.next()?;
    if !first.is_alphabetic() && first != '_' {
        return None;
    }
    end += first.len_utf8();
    for (offset, character) in characters {
        if character.is_alphanumeric() || character == '_' {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn find_c_like_declaration(line: &str) -> Option<Separator> {
    let comment_start = line.find("//").unwrap_or(line.len());
    let code = &line[..comment_start];
    let code_end = code.trim_end().len();
    let terminator = code_end.checked_sub(1)?;
    if code.as_bytes().get(terminator) != Some(&b';') {
        return None;
    }

    let body_end = code[..terminator].trim_end().len();
    let body = &code[..body_end];
    if body.is_empty()
        || body.contains(['(', ')', '[', ']', '{', '}', '=', '#'])
        || has_top_level_comma(body)
    {
        return None;
    }

    let declarator_start = body
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map(|(index, character)| index + character.len_utf8())?;
    let type_prefix = body[..declarator_start].trim_end();
    let declarator = body[declarator_start..].trim();
    if type_prefix.is_empty()
        || !is_c_like_declarator(declarator)
        || is_c_like_statement(type_prefix)
    {
        return None;
    }

    Some(Separator {
        kind: "c_like_declaration",
        range: declarator_start..body_end,
        whitespace_start: type_prefix.len(),
        trailing_whitespace_before_terminator: (body_end < terminator)
            .then_some(body_end..terminator),
    })
}

fn find_c_like_bitfield(line: &str) -> Option<Separator> {
    let comment_start = line.find("//").unwrap_or(line.len());
    let code = line[..comment_start].trim_end();
    let terminator = code.strip_suffix(';')?.trim_end();
    let colon = terminator.char_indices().find_map(|(offset, character)| {
        (character == ':'
            && !terminator[offset..].starts_with("::")
            && !terminator[..offset].ends_with(':'))
        .then_some(offset)
    })?;
    let lhs = terminator[..colon].trim_end();
    let rhs = terminator[colon + 1..].trim();
    if rhs.is_empty()
        || lhs.split_whitespace().count() < 2
        || lhs.contains(['(', ')', '{', '}', '='])
    {
        return None;
    }
    let whitespace_start = terminator[..colon]
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_whitespace())
        .map_or(0, |(index, character)| index + character.len_utf8());
    Some(Separator {
        kind: "c_like_bitfield",
        range: colon..colon + 1,
        whitespace_start,
        trailing_whitespace_before_terminator: None,
    })
}

fn find_go_field(line: &str) -> Option<Separator> {
    let uncommented = line.split("//").next()?;
    let leading = uncommented.len() - uncommented.trim_start().len();
    let code = uncommented.trim();
    if code.is_empty()
        || code.contains(['(', ')', '{', '}', '=', ':', ','])
        || ["return", "break", "continue", "goto", "defer", "go"]
            .into_iter()
            .any(|keyword| code.starts_with(keyword))
    {
        return None;
    }
    let first_end = identifier_end(code, 0)?;
    let whitespace_start = leading + first_end;
    let type_start = code[first_end..]
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map(|(offset, _)| first_end + offset)?;
    if type_start == first_end || code[type_start..].contains(char::is_whitespace) {
        return None;
    }
    Some(Separator {
        kind: "go_field",
        range: leading + type_start..leading + type_start,
        whitespace_start,
        trailing_whitespace_before_terminator: None,
    })
}

fn has_top_level_comma(text: &str) -> bool {
    let mut template_depth = 0usize;
    for character in text.chars() {
        match character {
            '<' => template_depth += 1,
            '>' => template_depth = template_depth.saturating_sub(1),
            ',' if template_depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn is_c_like_declarator(text: &str) -> bool {
    let text = text.trim_start_matches(['*', '&']);
    let mut characters = text.chars();
    matches!(characters.next(), Some(character) if character.is_alphabetic() || character == '_')
        && characters.all(|character| character.is_alphanumeric() || character == '_')
}

fn is_c_like_statement(type_prefix: &str) -> bool {
    let first_word = type_prefix
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .find(|word| !word.is_empty())
        .unwrap_or_default();
    matches!(
        first_word,
        "return"
            | "throw"
            | "delete"
            | "goto"
            | "co_return"
            | "typedef"
            | "using"
            | "class"
            | "struct"
            | "enum"
            | "union"
    )
}

fn reflow_signatures(
    text: &str,
    lines: &[Line<'_>],
    excluded_rows: &[bool],
    tab_size: usize,
    language_id: &str,
    scope: FormatScope,
) -> (Vec<bool>, Vec<TextEdit>) {
    let mut reflowed_rows = vec![false; lines.len()];
    let mut edits = Vec::new();

    for line in lines {
        if excluded_rows[line.row] || reflowed_rows[line.row] {
            continue;
        }
        let Some(open_in_line) = first_open_parenthesis(line.text) else {
            continue;
        };
        let open = line.start + open_in_line;
        let Some(close) = matching_parenthesis(text, open) else {
            continue;
        };
        let Some(close_row) = lines.iter().position(|candidate| {
            candidate.start <= close && close <= candidate.start + candidate.text.len()
        }) else {
            continue;
        };
        if excluded_rows[line.row..=close_row]
            .iter()
            .any(|excluded| *excluded)
            || !signature_intersects_scope(line.row, close_row, scope)
        {
            continue;
        }

        let close_line = lines[close_row];
        let prefix = &text[line.start..open];
        let suffix = &text[close + 1..close_line.start + close_line.text.len()];
        let next_line = lines.get(close_row + 1).map_or("", |line| line.text);
        if !looks_like_declaration(prefix, suffix, next_line, language_id) {
            continue;
        }

        let constructor_initializers = is_cpp_language(language_id)
            .then(|| collect_cpp_constructor_initializers(text, lines, close, close_row))
            .flatten();
        let end_row = constructor_initializers
            .as_ref()
            .map(|initializers| initializers.end_row)
            .unwrap_or(close_row);
        if excluded_rows[line.row..=end_row]
            .iter()
            .any(|excluded| *excluded)
        {
            continue;
        }
        let range_end = constructor_initializers
            .as_ref()
            .map(|initializers| initializers.range_end)
            .unwrap_or(close_line.start + close_line.text.len());
        let range = line.start..range_end;
        let Some(edit) = reflow_signature(
            line,
            range.clone(),
            prefix,
            suffix,
            &text[open + 1..close],
            &text[range],
            tab_size,
            language_id,
            constructor_initializers.as_ref(),
        ) else {
            continue;
        };
        for row in line.row..=end_row {
            reflowed_rows[row] = true;
        }
        edits.push(edit);
    }

    (reflowed_rows, edits)
}

#[derive(Debug, Clone)]
struct PythonKeywordArgument {
    name: String,
    value: String,
}

fn reflow_python_keyword_calls(
    text: &str,
    lines: &[Line<'_>],
    excluded_rows: &[bool],
    signature_rows: &[bool],
    tab_size: usize,
    language_id: &str,
    scope: FormatScope,
) -> (Vec<bool>, Vec<TextEdit>) {
    let mut reflowed_rows = vec![false; lines.len()];
    let mut edits = Vec::new();
    let mut planned_ranges = Vec::<Range<usize>>::new();
    if !language_id.eq_ignore_ascii_case("python") {
        return (reflowed_rows, edits);
    }

    for start_line in lines {
        if excluded_rows[start_line.row] || signature_rows[start_line.row] {
            continue;
        }
        for open_in_line in unquoted_open_parentheses(start_line.text).into_iter().rev() {
            let open = start_line.start + open_in_line;
            let Some(close) = matching_parenthesis(text, open) else {
                continue;
            };
            let close_row = line_for_offset(lines, close);
            if close_row <= start_line.row
                || excluded_rows[start_line.row..=close_row]
                    .iter()
                    .any(|excluded| *excluded)
                || signature_rows[start_line.row..=close_row]
                    .iter()
                    .any(|handled| *handled)
                || reflowed_rows[start_line.row..=close_row]
                    .iter()
                    .any(|handled| *handled)
                || !signature_intersects_scope(start_line.row, close_row, scope)
            {
                continue;
            }

            let close_line = lines[close_row];
            let range = start_line.start..close_line.start + close_line.text.len();
            if planned_ranges
                .iter()
                .any(|planned| planned.start < range.end && range.start < planned.end)
            {
                continue;
            }
            let original = &text[range.clone()];
            if original.contains('#') {
                continue;
            }

            let prefix = &text[start_line.start..open];
            if !is_python_keyword_call_prefix(prefix) {
                continue;
            }
            let Some(arguments) = parse_python_keyword_arguments(&text[open + 1..close]) else {
                continue;
            };
            if arguments.len() < 2 {
                continue;
            }

            let suffix = &text[close + 1..close_line.start + close_line.text.len()];
            let replacement =
                format_python_keyword_call(start_line, prefix, suffix, &arguments, tab_size);
            if replacement == original {
                continue;
            }

            for row in start_line.row..=close_row {
                reflowed_rows[row] = true;
            }
            planned_ranges.push(range.clone());
            edits.push(TextEdit { range, replacement });
        }
    }

    (reflowed_rows, edits)
}

fn unquoted_open_parentheses(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut openings = Vec::new();
    let mut offset = 0;
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == active_quote {
                quote = None;
            }
            offset += char_len(text, offset);
            continue;
        }
        if bytes[offset] == b'#' {
            break;
        }
        if matches!(bytes[offset], b'\'' | b'"' | b'`') {
            quote = Some(bytes[offset]);
        } else if bytes[offset] == b'(' {
            openings.push(offset);
        }
        offset += char_len(text, offset);
    }
    openings
}

fn is_python_keyword_call_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim();
    if prefix.is_empty()
        || [
            "def ",
            "async def ",
            "class ",
            "if ",
            "elif ",
            "while ",
            "for ",
            "with ",
        ]
        .into_iter()
        .any(|keyword| prefix.starts_with(keyword))
    {
        return false;
    }
    prefix
        .chars()
        .next_back()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
}

fn parse_python_keyword_arguments(text: &str) -> Option<Vec<PythonKeywordArgument>> {
    let mut arguments = Vec::new();
    let parameters = split_parameters(text);
    let parameter_count = parameters.len();
    for (index, parameter) in parameters.into_iter().enumerate() {
        let parameter = parameter.trim().trim_end_matches(',').trim();
        if parameter.is_empty() && index + 1 == parameter_count {
            continue;
        }
        if parameter.is_empty()
            || parameter.contains(['\n', '\r', '#'])
            || looks_like_python_raw_literal(parameter)
        {
            return None;
        }
        let (name, value) = split_python_keyword_argument(parameter)?;
        if value.contains(" for ") {
            return None;
        }
        arguments.push(PythonKeywordArgument {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
    Some(arguments)
}

fn looks_like_python_raw_literal(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("r\"")
        || text.starts_with("R\"")
        || text.starts_with("r'")
        || text.starts_with("R'")
}

fn split_python_keyword_argument(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if bytes[offset] == b'\\' {
                escaped = true;
            } else if bytes[offset] == active_quote {
                quote = None;
            }
            offset += char_len(text, offset);
            continue;
        }
        match bytes[offset] {
            b'\'' | b'"' | b'`' => quote = Some(bytes[offset]),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                let previous = offset.checked_sub(1).and_then(|index| bytes.get(index));
                let next = bytes.get(offset + 1);
                if matches!(previous, Some(b'=' | b'!' | b'<' | b'>')) || next == Some(&b'=') {
                    offset += 1;
                    continue;
                }
                let name = text[..offset].trim();
                let value = text[offset + 1..].trim();
                if name.is_empty()
                    || value.is_empty()
                    || !name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    return None;
                }
                return Some((name, value));
            }
            _ => {}
        }
        offset += char_len(text, offset);
    }
    None
}

fn format_python_keyword_call(
    start_line: &Line<'_>,
    prefix: &str,
    suffix: &str,
    arguments: &[PythonKeywordArgument],
    tab_size: usize,
) -> String {
    let prefix = prefix.trim_end();
    let indent = &start_line.text[..indentation(start_line.text)];
    let name_width = arguments
        .iter()
        .map(|argument| logical_column(&argument.name, tab_size))
        .max()
        .unwrap_or_default();
    let format_argument = |argument: &PythonKeywordArgument| {
        let width = logical_column(&argument.name, tab_size);
        format!(
            "{}{} = {}",
            argument.name,
            " ".repeat(name_width.saturating_sub(width) + 1),
            argument.value
        )
    };
    let first = format_argument(&arguments[0]);
    let continuation_indent = " ".repeat(logical_column(prefix, tab_size) + 1);
    let remaining = arguments[1..]
        .iter()
        .map(|argument| format!("{continuation_indent}{},", format_argument(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{prefix}({first},\n{remaining}\n{indent}){suffix}")
}

fn signature_intersects_scope(start: usize, end: usize, scope: FormatScope) -> bool {
    match scope {
        FormatScope::Document => true,
        FormatScope::Lines {
            start: selected_start,
            end: selected_end,
        } => start <= selected_end && selected_start <= end,
    }
}

fn reflow_signature(
    start_line: &Line<'_>,
    range: Range<usize>,
    prefix: &str,
    suffix: &str,
    parameter_text: &str,
    original: &str,
    tab_size: usize,
    language_id: &str,
    constructor_initializers: Option<&CppConstructorInitializers>,
) -> Option<TextEdit> {
    let mut parameters = split_parameters(parameter_text);
    if parameters.len() == 1
        && parameters[0].trim().is_empty()
        && constructor_initializers.is_some()
    {
        parameters.clear();
    } else if parameters
        .iter()
        .any(|parameter| parameter.trim().is_empty())
        || (parameters.len() < 2 && constructor_initializers.is_none())
    {
        return None;
    }
    for parameter in &mut parameters {
        *parameter = parameter.trim().trim_end_matches(',').trim().to_string();
    }

    align_parameters(&mut parameters, tab_size, language_id);
    let indent = &start_line.text[..start_line.text.len() - start_line.text.trim_start().len()];
    if is_c_like_language(language_id) {
        let signature = match parameters.as_slice() {
            [] => format!("{prefix}()"),
            [parameter] => format!("{prefix}({parameter})"),
            [first_parameter, remaining @ ..] => {
                let parameter_indent = " ".repeat(logical_column(prefix, tab_size) + 1);
                let remaining_parameters = remaining
                    .iter()
                    .enumerate()
                    .map(|(index, parameter)| {
                        let comma = (index + 1 < remaining.len()).then_some(",").unwrap_or("");
                        format!("{parameter_indent}{parameter}{comma}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{prefix}({first_parameter},\n{remaining_parameters})")
            }
        };
        let replacement = if let Some(initializers) = constructor_initializers {
            format!(
                "{signature}\n{}",
                format_cpp_constructor_initializers(initializers, indent, tab_size)
            )
        } else {
            format!("{signature}{suffix}")
        };
        return (replacement != original).then_some(TextEdit { range, replacement });
    }
    let continuation_indent = if indent.contains('\t') {
        format!("{indent}\t")
    } else {
        format!("{indent}{}", " ".repeat(tab_size.max(1)))
    };
    let trailing_comma = !is_c_like_language(language_id);
    let parameter_lines = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let comma = if trailing_comma || index + 1 < parameters.len() {
                ","
            } else {
                ""
            };
            format!("{continuation_indent}{parameter}{comma}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let replacement = format!("{prefix}(\n{parameter_lines}\n{indent}){suffix}");
    (replacement != original).then_some(TextEdit { range, replacement })
}

fn collect_cpp_constructor_initializers(
    text: &str,
    lines: &[Line<'_>],
    close: usize,
    close_row: usize,
) -> Option<CppConstructorInitializers> {
    let close_line = lines.get(close_row)?;
    let close_tail = &text[close + 1..close_line.start + close_line.text.len()];
    let (initializer_row, initializer_text) =
        if let Some(after_colon) = close_tail.trim_start().strip_prefix(':') {
            (close_row, after_colon)
        } else if close_tail.trim().is_empty() {
            let initializer_line = lines
                .iter()
                .skip(close_row + 1)
                .find(|line| !line.text.trim().is_empty())?;
            let after_colon = initializer_line.text.trim_start().strip_prefix(':')?;
            (initializer_line.row, after_colon)
        } else {
            return None;
        };

    let mut entries_text = String::new();
    let mut end_row = initializer_row;
    if !initializer_text.trim().is_empty() {
        entries_text.push_str(initializer_text.trim());
    }

    let mut found_body_brace = false;
    for line in lines.iter().skip(initializer_row + 1) {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "{" {
            found_body_brace = true;
            break;
        }
        if trimmed.contains("//") {
            return None;
        }
        if !entries_text.is_empty() {
            entries_text.push(' ');
        }
        entries_text.push_str(trimmed);
        end_row = line.row;
    }
    if !found_body_brace || entries_text.contains("//") {
        return None;
    }

    let entries = split_top_level_commas(&entries_text)
        .into_iter()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if entries.len() < 2
        || entries
            .iter()
            .any(|entry| find_cpp_initializer(entry).is_none())
    {
        return None;
    }

    let end_line = lines.get(end_row)?;
    Some(CppConstructorInitializers {
        end_row,
        range_end: end_line.start + end_line.text.len(),
        entries,
    })
}

fn format_cpp_constructor_initializers(
    initializers: &CppConstructorInitializers,
    header_indent: &str,
    tab_size: usize,
) -> String {
    let initializer_indent = if header_indent.contains('\t') {
        format!("{header_indent}\t")
    } else {
        format!("{header_indent}{}", " ".repeat(tab_size.max(1)))
    };
    let member_indent = format!("{initializer_indent}  ");
    initializers
        .entries
        .iter()
        .enumerate()
        .map(|(index, initializer)| {
            let comma = (index + 1 < initializers.entries.len())
                .then_some(",")
                .unwrap_or("");
            if index == 0 {
                format!("{initializer_indent}: {initializer}{comma}")
            } else {
                format!("{member_indent}{initializer}{comma}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_open_parenthesis(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in line.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == '(' {
            return Some(offset);
        }
    }
    None
}

fn matching_parenthesis(line: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in line[open..].char_indices() {
        let offset = open + offset;
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == '(' {
            depth += 1;
        } else if character == ')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

fn looks_like_declaration(prefix: &str, suffix: &str, next_line: &str, language_id: &str) -> bool {
    let prefix = prefix.trim();
    let suffix = suffix.trim_start();
    let next_line = next_line.trim_start();
    if prefix.starts_with("def ")
        || prefix.starts_with("fn ")
        || prefix.starts_with("func ")
        || prefix.starts_with("function ")
    {
        return true;
    }
    if suffix.starts_with("=>") {
        return prefix.contains('=');
    }
    if prefix.contains('.') || prefix.contains("->") || prefix.contains('=') {
        return false;
    }
    if is_c_like_language(language_id) && suffix.starts_with(';') {
        // `Type value(...)` is indistinguishable from a declaration without a
        // parser. Do not rewrite C/C++ construction expressions as functions.
        return false;
    }
    let follows_declaration = suffix.starts_with('{')
        || suffix.starts_with("->")
        || suffix.starts_with(':')
        || suffix.starts_with(';')
        || next_line.starts_with('{')
        || (is_cpp_language(language_id) && next_line.starts_with(':'));
    if !follows_declaration {
        return false;
    }
    let has_return_type = prefix.split_whitespace().count() >= 2;
    let cpp_constructor = is_cpp_language(language_id)
        && is_c_like_declarator(prefix)
        && (suffix.starts_with('{')
            || suffix.starts_with(':')
            || next_line.starts_with('{')
            || next_line.starts_with(':'));
    has_return_type || cpp_constructor
}

fn split_parameters(text: &str) -> Vec<String> {
    let mut parameters = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in text.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if matches!(character, '(' | '[' | '{' | '<') {
            depth += 1;
        } else if matches!(character, ')' | ']' | '}' | '>') {
            depth = depth.saturating_sub(1);
        } else if character == ',' && depth == 0 {
            parameters.push(text[start..offset].to_string());
            start = offset + 1;
        }
    }
    parameters.push(text[start..].to_string());
    parameters
}

fn align_parameters(parameters: &mut [String], tab_size: usize, language_id: &str) {
    align_parameter_separator(parameters, ":", tab_size);
    align_parameter_separator(parameters, "=", tab_size);
    if parameters
        .iter()
        .all(|parameter| find_named_separator(parameter, ":").is_none())
    {
        if language_id.eq_ignore_ascii_case("go") {
            align_go_parameter_types(parameters, tab_size);
        } else if is_c_like_language(language_id)
            && align_c_like_trailing_qualifiers(parameters, tab_size)
        {
        } else {
            align_trailing_parameter_names(parameters, tab_size);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CLikeTrailingQualifier {
    qualifier_start: usize,
    qualifier_whitespace_start: usize,
    declarator_end: usize,
    name_start: usize,
}

fn align_c_like_trailing_qualifiers(parameters: &mut [String], tab_size: usize) -> bool {
    let qualifiers = parameters
        .iter()
        .map(|parameter| find_c_like_trailing_qualifier(parameter))
        .collect::<Vec<_>>();
    if qualifiers.iter().flatten().count() < 2 {
        return false;
    }

    let target = qualifiers
        .iter()
        .enumerate()
        .filter_map(|(index, qualifier)| {
            qualifier.map(|qualifier| {
                logical_column(&parameters[index][..qualifier.qualifier_start], tab_size)
            })
        })
        .max()
        .unwrap_or_default();
    for (parameter, qualifier) in parameters.iter_mut().zip(qualifiers) {
        let Some(qualifier) = qualifier else {
            continue;
        };
        let column = logical_column(&parameter[..qualifier.qualifier_whitespace_start], tab_size);
        let qualifier_gap = " ".repeat(target.saturating_sub(column));
        let offset = qualifier_gap.len() as isize
            - (qualifier.qualifier_start - qualifier.qualifier_whitespace_start) as isize;
        parameter.replace_range(
            qualifier.qualifier_whitespace_start..qualifier.qualifier_start,
            &qualifier_gap,
        );
        let adjusted_name_start = qualifier.name_start.saturating_add_signed(offset);
        let adjusted_declarator_end = qualifier.declarator_end.saturating_add_signed(offset);
        parameter.replace_range(adjusted_declarator_end..adjusted_name_start, "");
    }
    true
}

fn find_c_like_trailing_qualifier(parameter: &str) -> Option<CLikeTrailingQualifier> {
    ["const", "volatile"]
        .into_iter()
        .rev()
        .find_map(|qualifier| {
            parameter
                .match_indices(qualifier)
                .filter_map(|(qualifier_start, _)| {
                    let qualifier_end = qualifier_start + qualifier.len();
                    let before = parameter[..qualifier_start].chars().next_back();
                    let after = parameter[qualifier_end..].chars().next();
                    if before
                        .is_some_and(|character| character.is_alphanumeric() || character == '_')
                        || after.is_some_and(|character| {
                            character.is_alphanumeric() || character == '_'
                        })
                    {
                        return None;
                    }
                    let qualifier_whitespace_start = parameter[..qualifier_start]
                        .char_indices()
                        .rev()
                        .find(|(_, character)| !character.is_whitespace())
                        .map_or(0, |(index, character)| index + character.len_utf8());
                    let mut declarator_start = qualifier_end;
                    declarator_start += parameter[declarator_start..].len()
                        - parameter[declarator_start..].trim_start().len();
                    if !parameter[declarator_start..].starts_with(['&', '*']) {
                        return None;
                    }
                    let mut declarator_end = declarator_start;
                    while parameter[declarator_end..].starts_with(['&', '*']) {
                        declarator_end += 1;
                    }
                    let mut name_start = declarator_end;
                    name_start +=
                        parameter[name_start..].len() - parameter[name_start..].trim_start().len();
                    parameter[name_start..]
                        .chars()
                        .next()
                        .filter(|character| character.is_alphabetic() || *character == '_')
                        .map(|_| CLikeTrailingQualifier {
                            qualifier_start,
                            qualifier_whitespace_start,
                            declarator_end,
                            name_start,
                        })
                })
                .last()
        })
}

fn align_parameter_separator(parameters: &mut [String], kind: &str, tab_size: usize) {
    let separators = parameters
        .iter()
        .map(|parameter| find_named_separator(parameter, kind))
        .collect::<Vec<_>>();
    if separators.iter().flatten().count() < 2 {
        return;
    }
    let target = separators
        .iter()
        .enumerate()
        .filter_map(|(index, separator)| {
            separator.as_ref().map(|(whitespace_start, _, _)| {
                logical_column(&parameters[index][..*whitespace_start], tab_size) + 1
            })
        })
        .max()
        .unwrap_or_default();
    for (parameter, separator) in parameters.iter_mut().zip(separators) {
        let Some((whitespace_start, separator_start, _)) = separator else {
            continue;
        };
        let column = logical_column(&parameter[..whitespace_start], tab_size);
        parameter.replace_range(
            whitespace_start..separator_start,
            &" ".repeat(target.saturating_sub(column)),
        );
    }
}

fn find_named_separator(text: &str, kind: &str) -> Option<(usize, usize, usize)> {
    let mut offset = 0;
    let bytes = text.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            offset += char_len(text, offset);
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
            offset += 1;
            continue;
        }
        if bytes[offset..].starts_with(kind.as_bytes()) {
            if kind == ":"
                && (matches!(bytes.get(offset + 1), Some(b':' | b'='))
                    || matches!(
                        offset.checked_sub(1).and_then(|index| bytes.get(index)),
                        Some(b':')
                    ))
            {
                offset += 1;
                continue;
            }
            let whitespace_start = text[..offset]
                .char_indices()
                .rev()
                .find(|(_, character)| !character.is_whitespace())
                .map_or(0, |(index, character)| index + character.len_utf8());
            return Some((whitespace_start, offset, offset + kind.len()));
        }
        offset += char_len(text, offset);
    }
    None
}

fn align_trailing_parameter_names(parameters: &mut [String], tab_size: usize) {
    let names = parameters
        .iter()
        .map(|parameter| last_identifier_start(parameter))
        .collect::<Vec<_>>();
    if names.iter().flatten().count() < 2 {
        return;
    }
    let target = names
        .iter()
        .enumerate()
        .filter_map(|(index, start)| {
            start.map(|start| logical_column(&parameters[index][..start], tab_size))
        })
        .max()
        .unwrap_or_default();
    for (parameter, name_start) in parameters.iter_mut().zip(names) {
        let Some(name_start) = name_start else {
            continue;
        };
        let whitespace_start = parameter[..name_start]
            .char_indices()
            .rev()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(0, |(index, character)| index + character.len_utf8());
        let column = logical_column(&parameter[..whitespace_start], tab_size);
        parameter.replace_range(
            whitespace_start..name_start,
            &" ".repeat(target.saturating_sub(column)),
        );
    }
}

fn align_go_parameter_types(parameters: &mut [String], tab_size: usize) {
    let type_starts = parameters
        .iter()
        .map(|parameter| {
            parameter
                .char_indices()
                .find(|(_, character)| character.is_whitespace())
                .and_then(|(index, _)| {
                    parameter[index..]
                        .char_indices()
                        .find(|(_, character)| !character.is_whitespace())
                        .map(|(offset, _)| index + offset)
                })
        })
        .collect::<Vec<_>>();
    if type_starts.iter().flatten().count() < 2 {
        return;
    }
    let target = type_starts
        .iter()
        .enumerate()
        .filter_map(|(index, start)| {
            start.map(|start| logical_column(&parameters[index][..start], tab_size))
        })
        .max()
        .unwrap_or_default();
    for (parameter, type_start) in parameters.iter_mut().zip(type_starts) {
        let Some(type_start) = type_start else {
            continue;
        };
        let whitespace_start = parameter[..type_start]
            .char_indices()
            .rev()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(0, |(index, character)| index + character.len_utf8());
        let column = logical_column(&parameter[..whitespace_start], tab_size);
        parameter.replace_range(
            whitespace_start..type_start,
            &" ".repeat(target.saturating_sub(column)),
        );
    }
}

fn last_identifier_start(text: &str) -> Option<usize> {
    let mut result = None;
    let mut in_identifier = false;
    for (offset, character) in text.char_indices() {
        if character.is_alphanumeric() || character == '_' {
            if !in_identifier {
                result = Some(offset);
                in_identifier = true;
            }
        } else {
            in_identifier = false;
        }
    }
    result
}

fn indentation(line: &str) -> usize {
    line.bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}

fn logical_column(text: &str, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    text.chars().fold(0, |column, character| {
        if character == '\t' {
            column + tab_size - column % tab_size
        } else {
            column + 1
        }
    })
}

fn char_len(line: &str, offset: usize) -> usize {
    line[offset..].chars().next().map_or(1, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, mut edits: Vec<TextEdit>) -> String {
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
        let mut result = text.to_string();
        for edit in edits {
            result.replace_range(edit.range, &edit.replacement);
        }
        result
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn document_aligns_each_block_independently() {
        let text = "a = 1\nlong_name = 2\n\nx = 3\nlonger_name = 4\n";
        let result = apply(text, format_document(text, "rust", 4));
        assert_eq!(
            result,
            "a           = 1\nlong_name   = 2\nx           = 3\nlonger_name = 4\n"
        );
    }

    #[test]
    fn range_only_changes_intersecting_block() {
        let text = "a = 1\nlong_name = 2\n\nx = 3\nlonger_name = 4\n";
        let result = apply(text, format_range(text, "rust", 4, 0, 0));
        assert_eq!(
            result,
            "a         = 1\nlong_name = 2\n\nx = 3\nlonger_name = 4\n"
        );
    }

    #[test]
    fn ignores_comments_strings_and_comparisons() {
        let text = "short = \"a: b\"\nlong_name = true\n// boundary\nvalid == expected\nx = false\nlonger = true\n";
        let result = apply(text, format_document(text, "cpp", 4));
        assert_eq!(
            result,
            "short     = \"a: b\"\nlong_name = true\n// boundary\nvalid == expected\nx      = false\nlonger = true\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn aligns_cpp_member_declarations() {
        let text = "    T *ptr; // allocated memory\n    int size;\n    int cap;\n\n    int a;\n    float b ;\n";
        let result = apply(text, format_document(text, "cpp", 4));
        assert_eq!(
            result,
            "    T     *ptr; // allocated memory\n    int   size;\n    int   cap;\n    int   a;\n    float b;\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn reflows_supported_function_declarations() {
        let cases = [
            (
                "def connect(host, retries, secure):",
                "python",
                "def connect(\n    host,\n    retries,\n    secure,\n):",
            ),
            (
                "pub fn connect(host: String, retries: u8, secure: bool) -> Result<()> {",
                "rust",
                "pub fn connect(\n    host    : String,\n    retries : u8,\n    secure  : bool,\n) -> Result<()> {",
            ),
            (
                "func connect(host string, retries int, secure bool) {",
                "go",
                "func connect(\n    host    string,\n    retries int,\n    secure  bool,\n) {",
            ),
            (
                "int connect(char *host, int retries, bool secure) {",
                "cpp",
                "int connect(\n    char *host,\n    int   retries,\n    bool  secure\n) {",
            ),
        ];
        for (source, language, expected) in cases {
            assert_eq!(
                apply(source, format_document(source, language, 4)),
                expected
            );
        }
    }

    #[test]
    fn leaves_calls_unchanged() {
        let text = "connect(host, retries, secure);";
        assert!(format_document(text, "typescript", 2).is_empty());
    }

    #[test]
    fn preserves_tab_indentation() {
        let text = "\tkey = value\n\tlonger = value\n";
        assert_eq!(
            apply(text, format_document(text, "go", 4)),
            "\tkey    = value\n\tlonger = value\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn aligns_cpp_templates_initializers_streams_and_multiline_constructors() {
        let declarations =
            "    std::vector<T> items;\n    std::unordered_map<int, size_t> index;\n";
        let declarations = apply(declarations, format_document(declarations, "cpp", 4));
        assert_eq!(
            declarations
                .lines()
                .map(|line| line
                    .find(if line.contains("items") {
                        "items"
                    } else {
                        "index"
                    })
                    .unwrap())
                .collect::<Vec<_>>(),
            vec![36, 36]
        );

        let constructor = "    Employee(int i,\n             std::string name,\n             double salary)\n    {\n";
        assert_eq!(
            apply(constructor, format_document(constructor, "cpp", 4)),
            "    Employee(\n        int         i,\n        std::string name,\n        double      salary\n    ) {\n"
        );

        let initializers =
            "        : id(i),\n        name(std::move(name)),\n        salary(salary)\n";
        assert_eq!(
            apply(initializers, format_document(initializers, "cpp", 4)),
            "        : id(i),\n          name(std::move(name)),\n          salary(salary)\n"
        );

        let streams = "        << \"ID=\" << id\n        << \"Employee name=\" << name\n        << \"Salary=\" << salary;\n";
        let streams = apply(streams, format_document(streams, "cpp", 4));
        let columns = streams
            .lines()
            .map(|line| line.rfind("<<").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(columns, vec![28, 28, 28]);
    }

    #[test]
    fn leaves_cpp_construction_and_multiline_calls_unchanged() {
        let text = "Company company(\n    \"Chaotic Systems\"\n);\nauto employee = find(\n    search_id,\n    retries\n);\n";
        assert!(format_document(text, "cpp", 4).is_empty());
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn normalizes_cpp_blocks_imports_and_continuations() {
        let text = "#include <vector>\n\n#include <string>\n\n\nvoid run() {\n    prepare();\n\n\n    if (ready) {\n        work();\n    }\n\n\n    finish();\n}\n\n\nint main() {\n    if (ready) {\n        run();\n    }\n\n    else {\n        recover();\n    }\n\n\n    return 0;\n}\n";
        assert_eq!(
            apply(text, format_document(text, "cpp", 4)),
            "#include <vector>\n#include <string>\n\nvoid run() {\n    prepare();\n    if (ready) {\n        work();\n    }\n\n    finish();\n}\n\nint main() {\n    if (ready) {\n        run();\n    }\n    else {\n        recover();\n    }\n\n    return 0;\n}\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn normalizes_python_suites_without_splitting_continuations() {
        let text = "import os\n\n\n\ndef run():\n    prepare()\n\n\n    if ready:\n        work()\n\n\n    else:\n        recover()\n\n\n    finish()\n\n\ndef main():\n    run()\n";
        assert_eq!(
            apply(text, format_document(text, "python", 4)),
            "import os\n\ndef run():\n    prepare()\n    if ready:\n        work()\n    else:\n        recover()\n\n    finish()\n\ndef main():\n    run()\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn keeps_data_braces_contiguous_and_finishes_do_while_blocks() {
        let text = "const options = {\n    enabled: true,\n};\n\n\ndo {\n    step();\n}\n\nwhile (ready);\nnext();\n";
        assert_eq!(
            apply(text, format_document(text, "typescript", 4)),
            "const options = {\n    enabled: true,\n};\ndo {\n    step();\n}\nwhile (ready);\n\nnext();\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn keeps_same_line_else_attached_and_separates_the_finished_block() {
        let text = "void run() {\nif (ready) {\nfirst();\n} else {\nsecond();\n}\nnext();\n}\n";
        assert_eq!(
            apply(text, format_document(text, "cpp", 4)),
            "void run() {\n    if (ready) {\n        first();\n    } else {\n        second();\n    }\n\n    next();\n}\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn preserves_native_continuation_indentation() {
        let text = "void run() {\n    std::cout << \"first\"\n        << \"second\";\n}\n";
        assert_eq!(format_document_text_after_native(text, "cpp", 4), text);
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn range_formatting_normalizes_the_touched_block() {
        let text =
            "void run() {\n    first();\n\n\n    second();\n}\n\n\nvoid next() {\n    run();\n}\n";
        assert_eq!(
            apply(text, format_range(text, "cpp", 4, 1, 1)),
            "void run() {\n    first();\n    second();\n}\n\nvoid next() {\n    run();\n}\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn range_formatting_remaps_rows_after_spacing_changes() {
        let text = "void run() {\n    long_name = 1;\n\n\n    x = 2;\n}\n";
        assert_eq!(
            apply(text, format_range(text, "cpp", 4, 4, 4)),
            "void run() {\n    long_name = 1;\n    x         = 2;\n}\n"
        );
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn normalizes_every_supported_brace_language() {
        let cases = [
            (
                "#include <stdio.h>\n\n\nvoid run() {\n    step();\n}\n\n\n// Next declaration.\nvoid next() {}\n",
                "c",
                "#include <stdio.h>\n\nvoid run() {\n    step();\n}\n\n// Next declaration.\nvoid next() {}\n",
            ),
            (
                "use crate::service;\n\n\nfn run() {\n    step();\n}\n\n\nfn next() {}\n",
                "rust",
                "use crate::service;\n\nfn run() {\n    step();\n}\n\nfn next() {}\n",
            ),
            (
                "import \"fmt\"\n\n\nfunc run() {\n    fmt.Println(\"run\")\n}\n\n\nfunc next() {}\n",
                "go",
                "import \"fmt\"\n\nfunc run() {\n    fmt.Println(\"run\")\n}\n\nfunc next() {}\n",
            ),
            (
                "import service from \"./service\";\n\n\nfunction run() {\n  service();\n}\n\n\nfunction next() {}\n",
                "javascript",
                "import service from \"./service\";\n\nfunction run() {\n    service();\n}\n\nfunction next() {}\n",
            ),
        ];
        for (source, language, expected) in cases {
            assert_eq!(
                apply(source, format_document(source, language, 4)),
                expected
            );
        }
    }

    #[test]
    #[ignore = "superseded by vertical-layout profile fixtures"]
    fn lays_out_cpp_templates_access_sections_and_completed_blocks() {
        let text = "struct Employee\n{\nint id;\nstd::string name;\nEmployee(int i, std::string n)\n{\n}\nvoid print(int value, int width)\n{\nif (value) {\nreturn;\n}\nelse {\nreturn;\n}\n}\n};\ntemplate<typename T> class Repository\n{\nprivate:\nstd::vector<T> items;\npublic:\nvoid add(T value, int count)\n{\n}\n};\n";
        assert_eq!(
            apply(text, format_document(text, "cpp", 4)),
            "struct Employee {\n    int         id;\n    std::string name;\n\n    Employee(\n        int         i,\n        std::string n\n    ) {\n    }\n\n    void print(\n        int value,\n        int width\n    ) {\n        if (value) {\n            return;\n        }\n        else {\n            return;\n        }\n    }\n};\n\ntemplate <typename T>\nclass Repository {\n    private:\n        std::vector<T> items;\n    public:\n        void add(\n            T   value,\n            int count\n        ) {\n        }\n};\n"
        );
    }

    #[test]
    fn lays_out_every_supported_language_without_changing_tokens() {
        let cases = [
            ("void run() {\nstep();\n}\n", "c"),
            ("fn run() {\nstep();\n}\n", "rust"),
            ("func run() {\nstep()\n}\n", "go"),
            ("function run() {\nstep();\n}\n", "javascript"),
            ("function run() {\nstep();\n}\n", "typescript"),
            ("def run():\nstep()\n", "python"),
        ];
        for (source, language) in cases {
            let formatted = apply(source, format_document(source, language, 4));
            assert!(formatted.contains("    step"), "{language}: {formatted}");
            assert!(formatted.contains("run"), "{language}: {formatted}");
        }
    }

    #[test]
    fn c_and_cpp_profiles_preserve_vertical_sections() {
        let text = "struct CpuFlags {\nuint32_t carry : 1;\nuint32_t interrupt : 1;\nuint32_t reserved : 30;\n};\n\nint main() {\nint min_workers = 4;\nint active = 16;\nuint64_t total_requests = 100;\nuint64_t completed = 80;\nactive += 2;\nqueue_depth *= 2;\nreturn 0;\n}\n";
        for language in ["c", "cpp"] {
            let formatted = apply(text, format_document(text, language, 4));
            assert!(
                formatted.starts_with("struct CpuFlags\n{\n"),
                "{language}: {formatted}"
            );
            let colon_columns = formatted
                .lines()
                .filter(|line| line.contains("uint32_t"))
                .map(|line| line.find(':').unwrap())
                .collect::<Vec<_>>();
            assert_eq!(colon_columns, vec![23, 23, 23], "{language}: {formatted}");
            assert!(
                formatted.contains("    int      min_workers")
                    && formatted.contains("    uint64_t total_requests"),
                "{language}: {formatted}"
            );
            assert!(
                formatted.contains("    uint64_t completed")
                    && formatted.contains("= 80;\n\n    active"),
                "{language}: {formatted}"
            );
        }
    }

    #[test]
    fn cpp_profile_expands_designated_initializers_and_stream_heads() {
        let text = "void run() {\nMemoryRegion code_region{.base_address = 1, .size_bytes = 2, .readable = true};\nstd::cout << \"Node: \" << node_name << '\\n';\n}\n";
        let formatted = apply(text, format_document(text, "cpp", 4));
        assert!(formatted.contains("void run()\n{\n"), "{formatted}");
        assert!(
            formatted.contains("MemoryRegion code_region {\n        .base_address = 1,\n        .size_bytes   = 2,\n        .readable     = true\n    };"),
            "{formatted}"
        );
        assert!(
            formatted.contains("    std::cout\n        << \"Node: \""),
            "{formatted}"
        );
    }

    #[test]
    fn cpp_profile_keeps_the_first_function_parameter_on_the_header() {
        let text = "template <typename T>\nT clamp_value(\nT value,\nT lower,\nT upper\n)\n{\nreturn value;\n}\n";
        assert_eq!(
            apply(text, format_document(text, "cpp", 4)),
            "template <typename T>\nT clamp_value(T value,\n              T lower,\n              T upper)\n{\n    return value;\n}\n"
        );
    }

    #[test]
    fn cpp_profile_aligns_qualifiers_and_keeps_reference_declarators_together() {
        let text = "double calculate_load_score(\nProcessStats const &stats,\nCpuFlags const &    flags\n)\n{\nreturn 0.0;\n}\n";
        assert_eq!(
            apply(text, format_document(text, "cpp", 4)),
            "double calculate_load_score(ProcessStats const &stats,\n                            CpuFlags     const &flags)\n{\n    return 0.0;\n}\n"
        );
    }

    #[test]
    fn cpp_profile_formats_constructor_initializer_lists() {
        let inline = "class WorkerRegistry {\npublic:\nWorkerRegistry(std::string service_name,\nint maximum_workers) : service_name_(std::move(service_name)), maximum_workers_(maximum_workers), accepted_(0), rejected_(0)\n{\n}\n};\n";
        let expected = "class WorkerRegistry\n{\n    public:\n        WorkerRegistry(std::string service_name,\n                       int         maximum_workers)\n            : service_name_(std::move(service_name)),\n              maximum_workers_(maximum_workers),\n              accepted_(0),\n              rejected_(0)\n        {\n        }\n};\n";
        let formatted = apply(inline, format_document(inline, "cpp", 4));
        assert_eq!(formatted, expected);
        assert_eq!(
            format_document_text(&formatted, "cpp", 4),
            formatted,
            "constructor formatting must be idempotent"
        );

        let already_multiline = "class WorkerRegistry\n{\npublic:\nWorkerRegistry(std::string service_name,\nint maximum_workers)\n:\nservice_name_(std::move(service_name)),\nmaximum_workers_(maximum_workers),\naccepted_(0),\nrejected_(0)\n{\n}\n};\n";
        assert_eq!(
            apply(
                already_multiline,
                format_document(already_multiline, "cpp", 4)
            ),
            expected
        );
    }

    #[test]
    fn cpp_constructor_initializer_lists_preserve_nested_calls_and_single_member_forms() {
        let nested = "struct WorkerRegistry {\nWorkerRegistry(std::string service_name) : service_name_(std::move(service_name)), maximum_workers_(limit_for(16, 2)), accepted_(0)\n{\n}\n};\n";
        let formatted = apply(nested, format_document(nested, "cpp", 4));
        assert!(
            formatted.contains(
                "    WorkerRegistry(std::string service_name)\n        : service_name_(std::move(service_name)),\n          maximum_workers_(limit_for(16, 2)),\n          accepted_(0)\n    {"
            ),
            "{formatted}"
        );

        let single_member = "struct WorkerRegistry {\nWorkerRegistry(int maximum_workers) : maximum_workers_(maximum_workers)\n{\n}\n};\n";
        let formatted = apply(single_member, format_document(single_member, "cpp", 4));
        assert!(
            formatted.contains(
                "    WorkerRegistry(int maximum_workers) : maximum_workers_(maximum_workers)\n    {"
            ),
            "{formatted}"
        );
    }

    #[test]
    fn cpp_profile_separates_aggregates_and_keeps_short_type_groups_together() {
        let text = "void run() {\nstd::string state = \"running\";\nMemoryRegion code_region{.base_address = 1,\n.size_bytes = 2,\n.readable = true};\nMemoryRegion heap_region{.base_address = 3, .size_bytes = 4, .readable = false};\nProcessStats stats{.pid = 42, .thread_count = 8, .cpu_percent = 75.0};\nconst double completion_ratio =\nstatic_cast<double>(completed) / total_requests;\nconst double failure_ratio = static_cast<double>(failed) / total_requests;\nconst double pending_ratio = static_cast<double>(pending) / total_requests;\nbool primary_added = registry.add(stats);\nbool backup_added = registry.add(backup_stats);\n\nProcessStats const *selected_worker = registry.find(stats.pid);\n\ndouble current_load_score = calculate_load_score(stats, flags);\n\nint recommended_workers = clamp_value(active, minimum, maximum);\n}\n";
        let formatted = apply(text, format_document(text, "cpp", 4));
        assert!(
            formatted.contains(
                "    std::string state = \"running\";\n\n    MemoryRegion code_region {\n        .base_address = 1,\n        .size_bytes   = 2,\n        .readable     = true\n    };\n\n    MemoryRegion heap_region {"
            ),
            "{formatted}"
        );
        let completion_ratio = formatted
            .lines()
            .find(|line| line.contains("completion_ratio"))
            .unwrap();
        assert!(
            completion_ratio.contains("= static_cast<double>(completed) / total_requests;"),
            "{formatted}"
        );
        assert!(
            formatted.contains(
                "    bool                 primary_added       = registry.add(stats);\n    bool                 backup_added        = registry.add(backup_stats);\n    ProcessStats const * selected_worker     = registry.find(stats.pid);\n    double               current_load_score  = calculate_load_score(stats, flags);\n    int                  recommended_workers = clamp_value(active, minimum, maximum);"
            ),
            "{formatted}"
        );
    }

    #[test]
    fn rust_go_and_python_profiles_align_their_native_field_forms() {
        let rust = "struct Stats {\nshort: u8,\nlonger_name: u16,\n}\n";
        assert_eq!(
            apply(rust, format_document(rust, "rust", 4)),
            "struct Stats {\n    short       : u8,\n    longer_name : u16,\n}\n"
        );

        let go = "type Config struct {\nHost string\nTimeoutMS time.Duration\n}\n";
        assert_eq!(
            apply(go, format_document(go, "go", 4)),
            "type Config struct {\n    Host      string\n    TimeoutMS time.Duration\n}\n"
        );

        let python = "class Config:\n    short: int = 1\n    longer_name: float = 2.0\n";
        assert_eq!(
            apply(python, format_document(python, "python", 4)),
            "class Config:\n    short       : int   = 1\n    longer_name : float = 2.0\n"
        );
    }

    #[test]
    fn python_profile_reflows_multiline_keyword_calls() {
        let source = "def build():\n    stats = ProcessStats(\n        pid=4217,\n        thread_count=worker_count,\n        virtual_memory=4 * MIB,\n    )\n    registry.add(\n        ProcessStats(\n            pid=4218,\n            thread_count=8,\n            virtual_memory=2 * MIB,\n        )\n    )\n    return stats\n";
        let formatted = apply(source, format_document(source, "python", 4));
        assert!(
            formatted.contains("    stats = ProcessStats(pid             = 4217,"),
            "{formatted}"
        );
        assert!(
            formatted.contains("                         thread_count    = worker_count,"),
            "{formatted}"
        );
        assert!(
            formatted.contains("        ProcessStats(pid             = 4218,"),
            "{formatted}"
        );
        assert!(
            formatted.contains("    )\n    registry.add("),
            "{formatted}"
        );
        assert!(formatted.contains("        )\n    )"), "{formatted}");

        let direct_equals = formatted
            .lines()
            .filter(|line| {
                line.contains("pid")
                    || line.contains("thread_count")
                    || line.contains("virtual_memory")
            })
            .take(3)
            .map(|line| line.rfind('=').unwrap())
            .collect::<Vec<_>>();
        assert!(
            direct_equals
                .windows(2)
                .all(|columns| columns[0] == columns[1]),
            "{formatted}"
        );
        assert_eq!(format_document_text(&formatted, "python", 4), formatted);

        let selected = apply(source, format_range(source, "python", 4, 3, 3));
        assert!(
            selected.contains("    stats = ProcessStats(pid             = 4217,"),
            "{selected}"
        );

        let legacy_indent = "def build():\n    registry.add(\n    ProcessStats(\n    pid=4218,\n    thread_count=8,\n    virtual_memory=2 * MIB,\n    )\n    )\n    return None\n";
        let repaired = apply(legacy_indent, format_document(legacy_indent, "python", 4));
        assert!(
            repaired.contains("        ProcessStats(pid             = 4218,"),
            "{repaired}"
        );
        assert!(
            repaired.contains("        )\n    )\n    return None"),
            "{repaired}"
        );
    }

    #[test]
    fn python_keyword_call_reflow_skips_compact_and_ambiguous_calls() {
        let source = "def build():\ncompact = ProcessStats(pid=1, thread_count=2)\npositional = ProcessStats(\n1,\n2,\n)\nspread = ProcessStats(\npid=1,\n**options,\n)\ncommented = ProcessStats(\n# leave this comment alone\npid=1,\nthread_count=2,\n)\n";
        let formatted = apply(source, format_document(source, "python", 4));
        assert!(
            formatted.contains("    compact = ProcessStats(pid=1, thread_count=2)"),
            "{formatted}"
        );
        assert!(
            !formatted.contains("pid          = 1"),
            "ambiguous calls must not receive keyword-call alignment: {formatted}"
        );
        assert!(
            formatted.contains("# leave this comment alone"),
            "{formatted}"
        );
    }

    #[test]
    fn rust_profile_attaches_attributes_preserves_operator_chains_and_aligns_fields() {
        let text = "#[derive(Debug, Clone)]\nstruct ProcessStats {\npid: i32,\nthread_count: u32,\nvirtual_memory: u64,\nresident_memory: u64,\ncpu_percent: f64,\nio_read_mb: f64,\nio_write_mb: f64,\n}\n\nfn checksum(&self) -> u64 {\nu64::from(self.control)\n^ u64::from(self.status)\n^ u64::from(self.interrupt_mask)\n^ u64::from(self.interrupt_status)\n^ self.dma_source\n^ self.dma_destination\n^ u64::from(self.dma_length)\n}\n\nfn summary() -> RegistrySummary<'static> {\nRegistrySummary {\nservice_name: \"packet-engine\",\naccepted: 2,\nrejected: 0,\nactive_workers: 2,\nutilization: 0.5,\n}\n}\n";
        let formatted = apply(text, format_document(text, "rust", 4));
        assert!(
            formatted.contains(
                "#[derive(Debug, Clone)]\nstruct ProcessStats {\n    pid             : i32,\n    thread_count    : u32,\n    virtual_memory  : u64,\n    resident_memory : u64,"
            ),
            "{formatted}"
        );
        assert!(
            formatted.contains(
                "u64::from(self.control)\n    ^ u64::from(self.status)\n    ^ u64::from(self.interrupt_mask)\n    ^ u64::from(self.interrupt_status)\n    ^ self.dma_source\n    ^ self.dma_destination\n    ^ u64::from(self.dma_length)"
            ),
            "{formatted}"
        );
        assert!(
            formatted.contains(
                "    RegistrySummary {\n        service_name   : \"packet-engine\",\n        accepted       : 2,\n        rejected       : 0,\n        active_workers : 2,\n        utilization    : 0.5,"
            ),
            "{formatted}"
        );
        assert_eq!(
            format_document_text(&formatted, "rust", 4),
            formatted,
            "Rust structural formatting must be idempotent"
        );
    }

    #[test]
    fn rust_profile_aligns_fields_after_a_lifetime_type() {
        let text = "#[derive(Debug, Clone, Copy)]\nstruct MemoryRegion {\nlabel: &'static str,\nbase_address: u64,\nsize_bytes: u64,\npermissions: u8,\nnuma_node: u16,\nreadable: bool,\nwritable: bool,\nexecutable: bool,\n}\n\nimpl MemoryRegion {\n}\n";
        let formatted = apply(text, format_document(text, "rust", 4));
        assert!(
            formatted.contains(
                "    label        : &'static str,\n    base_address : u64,\n    size_bytes   : u64,\n    permissions  : u8,\n    numa_node    : u16,\n    readable     : bool,\n    writable     : bool,\n    executable   : bool,"
            ),
            "{formatted}"
        );
        assert!(
            formatted.contains("}\n\nimpl MemoryRegion {"),
            "{formatted}"
        );
        assert_eq!(format_document_text(&formatted, "rust", 4), formatted);
    }

    #[test]
    fn removes_stale_attribute_and_operator_chain_gaps() {
        let rust = "#[derive(Debug, Clone, Copy, Default)]\n\nstruct CpuFlags {\ncarry: bool,\ninterrupt: bool,\nsupervisor: bool,\n}\n\nfn checksum(control: u32, status: u32, mask: u32) -> u32 {\nu32::from(control)\n\n^ u32::from(status)\n\n^ u32::from(mask)\n}\n";
        let formatted = apply(rust, format_document(rust, "rust", 4));
        assert!(
            formatted.contains(
                "#[derive(Debug, Clone, Copy, Default)]\nstruct CpuFlags {\n    carry      : bool,\n    interrupt  : bool,\n    supervisor : bool,"
            ),
            "{formatted}"
        );
        assert!(
            formatted
                .contains("u32::from(control)\n    ^ u32::from(status)\n    ^ u32::from(mask)"),
            "{formatted}"
        );
        assert_eq!(format_document_text(&formatted, "rust", 4), formatted);

        for (language, source) in [
            ("c", "int checksum(void) {\nvalue\n\n^ mask;\n}\n"),
            ("cpp", "int checksum() {\nvalue\n\n^ mask;\n}\n"),
            ("rust", "fn checksum() {\nvalue\n\n^ mask\n}\n"),
            ("go", "func checksum() {\nvalue\n\n^ mask\n}\n"),
            ("javascript", "function checksum() {\nvalue\n\n^ mask;\n}\n"),
            ("typescript", "function checksum() {\nvalue\n\n^ mask;\n}\n"),
        ] {
            let formatted = apply(source, format_document(source, language, 4));
            assert!(!formatted.contains("value\n\n"), "{language}: {formatted}");
        }

        let intentional_gap = "fn run() {\nlet first = 1;\n\nlet second = 2;\n}\n";
        let formatted = apply(intentional_gap, format_document(intentional_gap, "rust", 4));
        assert!(
            formatted.contains("let first = 1;\n\n    let second = 2;"),
            "{formatted}"
        );
    }

    #[test]
    fn rust_profile_aligns_load_score_parameters_and_assignments() {
        let text = "fn calculate_load_score(stats: &ProcessStats, flags: &CpuFlags) -> f64 {\nlet memory_gib = stats.resident_gib();\nlet thread_weight = f64::from(stats.thread_count) * 0.35;\nlet io_weight = stats.total_io_mb() * 0.01;\nlet interrupt_penalty = if flags.interrupt { 7.5 } else { 0.0 };\nlet supervisor_penalty = if flags.supervisor { 1.0 } else { 0.0 };\nlet overflow_penalty = if flags.overflow { 12.0 } else { 0.0 };\n}\n";
        let formatted = apply(text, format_document(text, "rust", 4));
        assert!(
            formatted.contains(
                "fn calculate_load_score(\n    stats : &ProcessStats,\n    flags : &CpuFlags,\n) -> f64 {"
            ),
            "{formatted}"
        );
        let assignment_columns = formatted
            .lines()
            .filter(|line| line.trim_start().starts_with("let "))
            .map(|line| line.find('=').unwrap())
            .collect::<Vec<_>>();
        assert!(
            assignment_columns.len() >= 6
                && assignment_columns
                    .windows(2)
                    .all(|columns| columns[0] == columns[1]),
            "{formatted}"
        );
    }

    #[test]
    fn aligns_static_print_labels_for_every_supported_language() {
        let cases = [
            (
                "rust",
                "println!(\"Node : {node}\");\neprintln!(\"Queue depth : {depth}\");\n",
                "println!(\"Node        : {node}\");\neprintln!(\"Queue depth : {depth}\");\n",
            ),
            (
                "c",
                "printf(\"Node : %s\\n\", node);\nfprintf(stderr, \"Queue depth : %zu\\n\", depth);\n",
                "printf(\"Node        : %s\\n\", node);\nfprintf(stderr, \"Queue depth : %zu\\n\", depth);\n",
            ),
            (
                "cpp",
                "std::println(\"Node : {}\", node);\nfprintf(stderr, \"Queue depth : %zu\\n\", depth);\n",
                "std::println(\"Node        : {}\", node);\nfprintf(stderr, \"Queue depth : %zu\\n\", depth);\n",
            ),
            (
                "go",
                "fmt.Printf(\"Node : %s\\n\", node)\nfmt.Fprintf(os.Stderr, \"Queue depth : %d\\n\", depth)\n",
                "fmt.Printf(\"Node        : %s\\n\", node)\nfmt.Fprintf(os.Stderr, \"Queue depth : %d\\n\", depth)\n",
            ),
            (
                "python",
                "print(f\"Node : {node}\")\nlogging.error(f\"Queue depth : {depth}\")\n",
                "print(f\"Node        : {node}\")\nlogging.error(f\"Queue depth : {depth}\")\n",
            ),
            (
                "typescript",
                "console.log(`Node : ${node}`);\nprocess.stderr.write(`Queue depth : ${depth}`);\n",
                "console.log(`Node        : ${node}`);\nprocess.stderr.write(`Queue depth : ${depth}`);\n",
            ),
        ];

        for (language, source, expected) in cases {
            let formatted = apply(source, format_document(source, language, 4));
            assert_eq!(formatted, expected, "{language}: {formatted}");
            assert_eq!(format_document_text(&formatted, language, 4), formatted);
        }
    }

    #[test]
    fn aligns_cpp_stream_labels_and_keeps_delimiter_groups_independent() {
        let streams =
            "std::cout << \"Node : \" << node;\nstd::cerr << \"Queue depth : \" << depth;\n";
        let formatted = apply(streams, format_document(streams, "cpp", 4));
        assert!(
            formatted.contains("<< \"Node        : \" << node;"),
            "{formatted}"
        );
        assert!(
            formatted.contains("std::cerr << \"Queue depth : \" << depth;"),
            "{formatted}"
        );

        let delimiters = "println!(\"Node : {node}\");\nprintln!(\"Queue depth : {depth}\");\nprintln!(\"PID = {pid}\");\nprintln!(\"Thread count = {threads}\");\nprintln!(\"In | {input}\");\nprintln!(\"Output value | {output}\");\n";
        let formatted = apply(delimiters, format_document(delimiters, "rust", 4));
        assert!(
            formatted.contains("\"Node        : {node}\""),
            "{formatted}"
        );
        assert!(
            formatted.contains("\"Queue depth : {depth}\""),
            "{formatted}"
        );
        assert!(
            formatted.contains("\"PID          = {pid}\""),
            "{formatted}"
        );
        assert!(
            formatted.contains("\"Thread count = {threads}\""),
            "{formatted}"
        );
        assert!(
            formatted.contains("\"In           | {input}\""),
            "{formatted}"
        );
        assert!(
            formatted.contains("\"Output value | {output}\""),
            "{formatted}"
        );
    }

    #[test]
    fn print_label_alignment_skips_ambiguous_literals_and_honors_ranges() {
        let guarded = "println!(format_string, node);\nreport(\"Node : {node}\");\nprintln!(\"URL: https://example.test\");\nprintln!(\"Value \\: {value}\");\nprintln!(\"Node : {node}\");\n\nprintln!(\"Queue depth : {depth}\");\n";
        assert_eq!(
            apply(guarded, format_document(guarded, "rust", 4)),
            guarded,
            "ambiguous or non-contiguous output must not be rewritten"
        );

        let source = "println!(\"Node : {node}\");\neprintln!(\"Queue depth : {depth}\");\n";
        let formatted = apply(source, format_range(source, "rust", 4, 0, 0));
        assert_eq!(
            formatted,
            "println!(\"Node        : {node}\");\neprintln!(\"Queue depth : {depth}\");\n"
        );
    }

    #[test]
    fn preserves_existing_major_blank_line_runs() {
        let text = "int first = 1;\n\n\nint second = 2;\n";
        let formatted = apply(text, format_document(text, "cpp", 4));
        assert_eq!(formatted.matches("\n\n\n").count(), 1, "{formatted}");
    }

    #[test]
    #[ignore = "run via scripts/benchmark.sh"]
    fn benchmark_assignment_blocks() {
        let text = (0..10_000)
            .map(|row| format!("field_{row} = value\n"))
            .collect::<String>();
        let started = std::time::Instant::now();
        let edits = format_document(&text, "rust", 4);
        eprintln!(
            "planned {} assignment edits for 10,000 rows in {:?}",
            edits.len(),
            started.elapsed()
        );
        assert_eq!(edits.len(), 1);
    }

    #[test]
    #[ignore = "run via scripts/benchmark.sh"]
    fn benchmark_cpp_declarations() {
        let text = (0..10_000)
            .map(|row| {
                if row % 2 == 0 {
                    format!("T field_{row};\n")
                } else {
                    format!("LongerType field_{row};\n")
                }
            })
            .collect::<String>();
        let started = std::time::Instant::now();
        let edits = format_document(&text, "cpp", 4);
        eprintln!(
            "planned {} C++ declaration edits for 10,000 rows in {:?}",
            edits.len(),
            started.elapsed()
        );
        assert_eq!(edits.len(), 1);
    }
}
