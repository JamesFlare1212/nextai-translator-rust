use super::{
    EMPTY, FILTERED, INCOMPLETE, MALFORMED, MAX_RESPONSE, PROVIDER_ERROR, REFUSED, TOO_LARGE,
    TRUNCATED,
};
use async_openai::types::responses::{
    OutputContent, OutputItem, OutputMessageContent, OutputStatus, Response, ResponseStreamEvent,
    Status,
};

#[derive(Default)]
pub(super) struct Responses {
    delivered: String,
}

fn incomplete(response: &Response) -> &'static str {
    match response
        .incomplete_details
        .as_ref()
        .map(|d| d.reason.as_str())
    {
        Some("max_output_tokens") => TRUNCATED,
        Some("content_filter") => FILTERED,
        _ => INCOMPLETE,
    }
}

fn check_refusal(item: &OutputItem) -> Result<(), String> {
    if let OutputItem::Message(message) = item
        && message
            .content
            .iter()
            .any(|part| matches!(part, OutputMessageContent::Refusal(_)))
    {
        return Err(REFUSED.into());
    }
    Ok(())
}

impl Responses {
    fn completed(&self, response: &Response) -> Result<bool, String> {
        if response.error.is_some() || response.status == Status::Failed {
            return Err(PROVIDER_ERROR.into());
        }
        if response.incomplete_details.is_some() || response.status == Status::Incomplete {
            return Err(incomplete(response).into());
        }
        if response.status != Status::Completed || response.object != "response" {
            return Err(MALFORMED.into());
        }
        let mut remaining = self.delivered.as_str();
        for item in &response.output {
            if let OutputItem::Message(message) = item {
                if message.status != OutputStatus::Completed {
                    return Err(INCOMPLETE.into());
                }
                for part in &message.content {
                    match part {
                        OutputMessageContent::Refusal(_) => return Err(REFUSED.into()),
                        OutputMessageContent::OutputText(part) => {
                            remaining = remaining.strip_prefix(&part.text).ok_or(MALFORMED)?;
                        }
                    }
                }
            }
        }
        if !remaining.is_empty() {
            return Err(MALFORMED.into());
        }
        if self.delivered.is_empty() {
            return Err(EMPTY.into());
        }
        Ok(true)
    }

    pub(super) fn event(
        &mut self,
        event: ResponseStreamEvent,
        mut emit: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<bool, String> {
        use ResponseStreamEvent::*;
        match event {
            ResponseOutputTextDelta(e) if !e.delta.is_empty() => {
                if e.delta.len() > MAX_RESPONSE - self.delivered.len() {
                    return Err(TOO_LARGE.into());
                }
                emit(&e.delta)?;
                self.delivered.push_str(&e.delta);
            }
            ResponseRefusalDelta(_) | ResponseRefusalDone(_) => return Err(REFUSED.into()),
            ResponseContentPartAdded(e) if matches!(e.part, OutputContent::Refusal(_)) => {
                return Err(REFUSED.into());
            }
            ResponseContentPartDone(e) if matches!(e.part, OutputContent::Refusal(_)) => {
                return Err(REFUSED.into());
            }
            ResponseOutputItemAdded(e) => check_refusal(&e.item)?,
            ResponseOutputItemDone(e) => check_refusal(&e.item)?,
            ResponseFailed(_) | ResponseError(_) => return Err(PROVIDER_ERROR.into()),
            ResponseIncomplete(e) => return Err(incomplete(&e.response).into()),
            ResponseCompleted(e) => return self.completed(&e.response),
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn completed(status: &str, text: &str) -> ResponseStreamEvent {
        serde_json::from_value(json!({
            "type":"response.completed", "sequence_number":3, "response":{
                "object":"response", "id":"r", "model":"m", "created_at":1,"status":status,
                "output":[{"type":"message", "id":"msg", "role":"assistant", "status":"completed",
                    "content":[{"type":"output_text", "annotations":[], "text":text}]}]
            }
        }))
        .unwrap()
    }

    #[test]
    fn completion_confirms_but_never_repairs_streamed_text() {
        let mut r = Responses::default();
        let delta = serde_json::from_value(json!({
            "type":"response.output_text.delta", "sequence_number":1, "output_index":0,
            "item_id":"msg", "content_index":0, "delta":"译"
        }))
        .unwrap();
        let mut visible = String::new();
        assert!(
            !r.event(delta, |s| {
                visible.push_str(s);
                Ok(())
            })
            .unwrap()
        );
        for (status, text, expected) in [
            ("completed", "译", Ok(true)),
            ("completed", "译文", Err(MALFORMED.to_string())),
            ("completed", "另", Err(MALFORMED.to_string())),
            ("completed", "", Err(MALFORMED.to_string())),
            ("in_progress", "译", Err(MALFORMED.to_string())),
            ("failed", "译", Err(PROVIDER_ERROR.to_string())),
        ] {
            assert_eq!(
                r.event(completed(status, text), |_| panic!(
                    "snapshot must not emit text"
                )),
                expected
            );
        }
        assert_eq!(visible, "译");
    }
}
