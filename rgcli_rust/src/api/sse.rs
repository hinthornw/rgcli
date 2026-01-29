use bytes::Bytes;
use futures_util::{Stream, StreamExt, TryStreamExt};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;

#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: String,
}

pub async fn parse_sse<S, F>(stream: S, mut callback: F) -> anyhow::Result<()>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
    F: FnMut(SseEvent),
{
    let stream = stream.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));
    let reader = StreamReader::new(stream);
    let mut lines = FramedRead::new(reader, LinesCodec::new());

    let mut current = SseEvent::default();
    let mut data_lines: Vec<String> = Vec::new();

    while let Some(line) = lines.next().await {
        let line = line?;
        if line.is_empty() {
            if !data_lines.is_empty() {
                current.data = data_lines.join("\n");
                callback(current.clone());
            }
            current = SseEvent::default();
            data_lines.clear();
            continue;
        }

        if let Some(rest) = line.strip_prefix("event:") {
            current.event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            let data = if rest.starts_with(' ') {
                &rest[1..]
            } else {
                rest
            };
            data_lines.push(data.to_string());
        } else if let Some(rest) = line.strip_prefix("id:") {
            current.id = rest.trim().to_string();
        }
    }

    if !data_lines.is_empty() {
        current.data = data_lines.join("\n");
        callback(current);
    }

    Ok(())
}

pub fn is_end_event(event: &SseEvent) -> bool {
    matches!(event.event.as_str(), "end" | "done")
}

pub fn is_message_event(event: &SseEvent) -> bool {
    matches!(event.event.as_str(), "messages" | "data")
}
