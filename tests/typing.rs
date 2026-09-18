use tower_lsp::lsp_types::Position;
use wtf::{actions, document::Document, typing};

fn after(text: &str, line: u32, character: u32, ch: &str) -> String {
    let doc = Document::parse(text.into());
    let edits = typing::on_type(&doc, Position::new(line, character), ch);
    actions::apply_edits(text, &edits).unwrap()
}

#[test]
fn typing_a_closing_pipe_realigns_the_table_without_touching_cell_text() {
    let source = "[t] := table\n| item | qty |\n|---|---|\n| apple | 2 |\n| watermelon | 10 |\n";
    // The last pipe of the final row was just typed; the caret sits after it.
    let doc = Document::parse(source.into());
    let edits = typing::on_type(&doc, Position::new(4, 19), "|");
    assert!(!edits.is_empty());
    // The typed row gets whitespace-only edits so the caret keeps its place.
    assert!(
        edits
            .iter()
            .filter(|e| e.range.start.line == 4)
            .all(|e| e.new_text.trim().is_empty())
    );
    let formatted = actions::apply_edits(source, &edits).unwrap();
    assert_eq!(
        formatted,
        "[t] := table\n| item       | qty |\n| ---------- | --- |\n| apple      | 2   |\n| watermelon | 10  |\n"
    );
    assert!(wtf::tables::formatting(&Document::parse(formatted)).is_empty());
}

#[test]
fn a_row_still_being_typed_is_left_alone_but_the_rest_of_the_table_aligns() {
    let source = "[t] := table\n| item | qty |\n|---|---|\n| apple | 2 |\n| pear |\n";
    // Pipe typed after "pear "; more cells are coming.
    let formatted = after(source, 4, 8, "|");
    assert_eq!(
        formatted,
        "[t] := table\n| item  | qty |\n| ----- | --- |\n| apple | 2   |\n| pear |\n"
    );
}

#[test]
fn pipes_outside_tables_and_other_characters_do_nothing() {
    let source = "Plain | prose\n- [ ] task\n";
    assert_eq!(after(source, 0, 7, "|"), source);
    assert_eq!(after(source, 1, 5, "x"), source);
}

#[test]
fn enter_after_a_task_continues_the_checklist_with_its_indent() {
    let source = "# List\n  - [x] done :done\n\n";
    // Enter was pressed at the end of line 1; the client left a blank line 2.
    assert_eq!(
        after(source, 2, 0, "\n"),
        "# List\n  - [x] done :done\n  - [ ] \n"
    );
    // Client auto-indent already inserted the leading spaces on the new line.
    let indented = "* [ ] first\n  \n";
    assert_eq!(after(indented, 1, 2, "\n"), "* [ ] first\n* [ ] \n");
}

#[test]
fn enter_on_an_empty_checkbox_ends_the_list() {
    let source = "- [ ] first\n- [ ] \n\n";
    assert_eq!(after(source, 2, 0, "\n"), "- [ ] first\n\n");
}

#[test]
fn enter_after_prose_or_with_text_after_the_caret_leaves_the_note_alone() {
    let prose = "Just words\n\n";
    assert_eq!(after(prose, 1, 0, "\n"), prose);
    let split = "- [ ] ab\ncd\n";
    // Caret after "c": the user is editing inside the new line, not continuing a list.
    assert_eq!(after(split, 1, 1, "\n"), split);
}
