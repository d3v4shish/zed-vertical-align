use std::collections::HashMap;

use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, DocumentRangeFormattingParams, InitializeParams, InitializeResult,
    InitializedParams, MessageType, OneOf, Position, Range, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};
use vertical_align_core::{format_document_text, format_range, TextEdit as CoreTextEdit};

#[cfg(test)]
use vertical_align_core::format_document;

const SERVER_NAME: &str = "zed-vertical-align-lsp";

#[derive(Clone)]
struct Document {
    language_id: String,
    text: String,
}

struct Backend {
    client: Client,
    documents: Mutex<HashMap<Url, Document>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    async fn document(&self, uri: &Url) -> Option<Document> {
        self.documents.lock().await.get(uri).cloned()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: formatting_capabilities(),
            server_info: Some(ServerInfo {
                name: SERVER_NAME.to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Zed Vertical Align formatter ready")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        self.documents.lock().await.insert(
            document.uri,
            Document {
                language_id: document.language_id,
                text: document.text,
            },
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.last() else {
            return;
        };
        if let Some(document) = self
            .documents
            .lock()
            .await
            .get_mut(&params.text_document.uri)
        {
            document.text = change.text.clone();
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .lock()
            .await
            .remove(&params.text_document.uri);
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(document) = self.document(&params.text_document.uri).await else {
            return Ok(None);
        };
        let tab_size = params.options.tab_size as usize;
        let formatted = format_document_text(&document.text, &document.language_id, tab_size);
        Ok(Some(whole_document_edit(&document.text, formatted)))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let Some(document) = self.document(&params.text_document.uri).await else {
            return Ok(None);
        };
        let (start, end) = requested_lines(&params.range);
        Ok(Some(to_lsp_edits(
            &document.text,
            format_range(
                &document.text,
                &document.language_id,
                params.options.tab_size as usize,
                start,
                end,
            ),
        )))
    }
}

fn formatting_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

fn requested_lines(range: &Range) -> (usize, usize) {
    let start = range.start.line as usize;
    let mut end = range.end.line as usize;
    if end > start && range.end.character == 0 {
        end -= 1;
    }
    (start, end.max(start))
}

fn to_lsp_edits(text: &str, edits: Vec<CoreTextEdit>) -> Vec<TextEdit> {
    edits
        .into_iter()
        .map(|edit| TextEdit {
            range: Range::new(
                byte_offset_to_position(text, edit.range.start),
                byte_offset_to_position(text, edit.range.end),
            ),
            new_text: edit.replacement,
        })
        .collect()
}

fn whole_document_edit(text: &str, replacement: String) -> Vec<TextEdit> {
    if replacement == text {
        Vec::new()
    } else {
        vec![TextEdit {
            range: Range::new(
                Position::new(0, 0),
                byte_offset_to_position(text, text.len()),
            ),
            new_text: replacement,
        }]
    }
}

fn byte_offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;
    for value in text[..offset.min(text.len())].chars() {
        if value == '\n' {
            line += 1;
            character = 0;
        } else {
            character += value.len_utf16() as u32;
        }
    }
    Position::new(line, character)
}

#[tokio::main]
async fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument != "--stdio") {
        eprintln!("usage: {SERVER_NAME} --stdio");
        std::process::exit(2);
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_only_formatting_capabilities() {
        let capabilities = formatting_capabilities();
        assert_eq!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        );
        assert_eq!(
            capabilities.document_formatting_provider,
            Some(OneOf::Left(true))
        );
        assert_eq!(
            capabilities.document_range_formatting_provider,
            Some(OneOf::Left(true))
        );
    }

    #[test]
    fn edits_use_utf16_positions() {
        let text = "😀x = 1\nlong_name = 2\n";
        let edits = to_lsp_edits(text, format_document(text, "rust", 4));
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert_eq!(edits[0].range.end, Position::new(2, 0));
        assert_eq!(
            byte_offset_to_position(text, "😀x".len()),
            Position::new(0, 3)
        );
    }

    #[test]
    fn range_end_at_line_start_is_exclusive() {
        assert_eq!(
            requested_lines(&Range::new(Position::new(2, 0), Position::new(4, 0))),
            (2, 3)
        );
    }

    #[test]
    fn whole_document_edit_uses_the_original_utf16_range() {
        let edits = whole_document_edit("😀x\n", "formatted\n".to_string());
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert_eq!(edits[0].range.end, Position::new(1, 0));
        assert_eq!(edits[0].new_text, "formatted\n");
    }
}
