use super::{FILTERED, INCOMPLETE, MALFORMED, PROVIDER_ERROR, REFUSED, TRUNCATED};
use serde::{Deserialize, de::IgnoredAny};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Responses {
    delivered: String,
    part_sizes: HashMap<(usize, usize), usize>,
}

#[derive(Deserialize)]
struct EventType {
    r#type: Option<String>,
    error: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct Event {
    #[serde(default)]
    output_index: usize,
    #[serde(default)]
    content_index: usize,
    delta: Option<String>,
    text: Option<String>,
    response: Option<Document>,
    part: Option<Part>,
    item: Option<Item>,
}

#[derive(Deserialize)]
struct Document {
    error: Option<IgnoredAny>,
    status: Option<String>,
    incomplete_details: Option<Incomplete>,
    output: Option<Vec<Item>>,
    output_text: Option<String>,
}

#[derive(Deserialize)]
struct Incomplete {
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Item {
    Message {
        role: Option<String>,
        content: Vec<Part>,
    },
    #[serde(other)]
    Other,
}

impl Item {
    fn parts(&self) -> &[Part] {
        match self {
            Self::Message { role, content } if role.as_deref().is_none_or(|r| r == "assistant") => {
                content
            }
            _ => &[],
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Part {
    OutputText {
        text: String,
    },
    Refusal,
    #[serde(other)]
    Other,
}

impl Part {
    fn visible(&self) -> Result<Option<&str>, String> {
        match self {
            Self::OutputText { text } => Ok(Some(text)),
            Self::Refusal => Err(REFUSED.into()),
            Self::Other => Ok(None),
        }
    }
}

impl Document {
    fn incomplete_error(&self) -> &'static str {
        match self
            .incomplete_details
            .as_ref()
            .and_then(|d| d.reason.as_deref())
        {
            Some("max_output_tokens") => TRUNCATED,
            Some("content_filter") => FILTERED,
            _ => INCOMPLETE,
        }
    }

    fn final_text(self) -> Result<String, String> {
        if self.error.is_some() || self.status.as_deref() == Some("failed") {
            return Err(PROVIDER_ERROR.into());
        }
        if self.status.as_deref().is_some_and(|s| s != "completed") {
            return Err(self.incomplete_error().into());
        }
        let mut text = String::new();
        for item in self.output.unwrap_or_default() {
            for part in item.parts() {
                if let Some(visible) = part.visible()? {
                    text.push_str(visible);
                }
            }
        }
        if text.is_empty() {
            Ok(self.output_text.unwrap_or_default())
        } else {
            Ok(text)
        }
    }
}

impl Responses {
    pub(super) fn decode(
        &mut self,
        bytes: &[u8],
        label: Option<&[u8]>,
        mut emit: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<bool, String> {
        let Some(label) = label else {
            let document: Document = serde_json::from_slice(bytes).map_err(|_| MALFORMED)?;
            emit(&document.final_text()?)?;
            return Ok(true);
        };
        let tag: EventType = serde_json::from_slice(bytes).map_err(|_| MALFORMED)?;
        if tag.error.is_some() {
            return Err(PROVIDER_ERROR.into());
        }
        let label = std::str::from_utf8(label).map_err(|_| MALFORMED)?;
        let kind = tag.r#type.as_deref().unwrap_or(label);
        if label.starts_with("response.") && label != kind {
            return Err(MALFORMED.into());
        }
        match kind {
            "error" | "response.failed" => return Err(PROVIDER_ERROR.into()),
            "response.refusal.delta" | "response.refusal.done" => return Err(REFUSED.into()),
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.content_part.done"
            | "response.output_item.done"
            | "response.completed"
            | "response.incomplete" => {}
            _ => return Ok(false),
        }
        let event: Event = serde_json::from_slice(bytes).map_err(|_| MALFORMED)?;
        if kind == "response.incomplete" {
            return Err(event
                .response
                .as_ref()
                .map_or(INCOMPLETE, Document::incomplete_error)
                .into());
        }
        if kind == "response.completed" {
            if let Some(document) = event.response {
                let text = document.final_text()?;
                if !text.is_empty() {
                    emit(text.strip_prefix(&self.delivered).ok_or(MALFORMED)?)?;
                }
            }
            return Ok(true);
        }
        let mut part = |index: usize, text: &str, snapshot: bool| -> Result<(), String> {
            if !text.is_empty() {
                let size = self
                    .part_sizes
                    .entry((event.output_index, index))
                    .or_default();
                let suffix = if snapshot {
                    text.get(*size..).ok_or(MALFORMED)?
                } else {
                    text
                };
                emit(suffix)?;
                self.delivered.push_str(suffix);
                *size += suffix.len();
            }
            Ok(())
        };
        match kind {
            "response.output_text.delta" => {
                part(event.content_index, &event.delta.ok_or(MALFORMED)?, false)?
            }
            "response.output_text.done" => {
                part(event.content_index, &event.text.ok_or(MALFORMED)?, true)?
            }
            "response.content_part.done" => {
                if let Some(text) = event.part.ok_or(MALFORMED)?.visible()? {
                    part(event.content_index, text, true)?;
                }
            }
            "response.output_item.done" => {
                for (index, content) in event.item.ok_or(MALFORMED)?.parts().iter().enumerate() {
                    if let Some(text) = content.visible()? {
                        part(index, text, true)?;
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(false)
    }
}
