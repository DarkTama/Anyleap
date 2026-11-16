use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::phone_files::ParsedNotificationRecord;
use crate::phone_hub::apps::resolve_app_label_cached;
use crate::phone_hub::{validate_serial, PhoneHubState};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub sender: String,
    pub text: String,
    pub timestamp_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub package: String,
    pub app_label: String,
    pub key: String,
    pub title: String,
    pub is_group: bool,
    pub messages: Vec<ChatMessage>,
    pub unread: u32,
}

#[derive(Default)]
pub struct MessageStore {
    by_serial: HashMap<String, HashMap<String, Conversation>>,
}

const MAX_MESSAGES_PER_CONVERSATION: usize = 200;

impl MessageStore {
    pub(crate) fn ingest(
        &mut self,
        serial: &str,
        records: &[ParsedNotificationRecord],
        labels: &HashMap<String, String>,
    ) -> Vec<String> {
        let convs = self.by_serial.entry(serial.to_string()).or_default();
        let mut changed = Vec::new();

        for rec in records {
            if !rec.is_messaging {
                continue;
            }

            let conv_id = rec
                .shortcut
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| rec.conversation_title.as_deref().filter(|s| !s.is_empty()))
                .or_else(|| (!rec.base.title.is_empty()).then_some(rec.base.title.as_str()))
                .unwrap_or(rec.base.key.as_str());
            let key = format!("{}::{conv_id}", rec.base.package);

            let app_label = labels
                .get(&rec.base.package)
                .cloned()
                .or_else(|| rec.base.app_label.clone())
                .unwrap_or_else(|| rec.base.package.clone());

            let title = rec
                .conversation_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| (!rec.base.title.is_empty()).then_some(rec.base.title.as_str()))
                .unwrap_or(&app_label)
                .to_string();

            let mut incoming: Vec<ChatMessage> = if !rec.messages.is_empty() {
                rec.messages
                    .iter()
                    .map(|m| ChatMessage {
                        sender: m.sender.clone(),
                        text: m.text.clone(),
                        timestamp_ms: m.timestamp_ms,
                    })
                    .collect()
            } else if !rec.base.text.is_empty() {
                let (sender, text) = if rec.is_group || rec.conversation_title.is_some() {
                    if let Some((s, t)) = rec.base.text.split_once(": ") {
                        (s.trim().to_string(), t.to_string())
                    } else {
                        (title.clone(), rec.base.text.clone())
                    }
                } else {
                    (title.clone(), rec.base.text.clone())
                };
                vec![ChatMessage {
                    sender,
                    text,
                    timestamp_ms: rec.base.posted_unix.saturating_mul(1000),
                }]
            } else {
                Vec::new()
            };

            if incoming.is_empty() {
                continue;
            }

            let conv = convs.entry(key.clone()).or_insert_with(|| Conversation {
                package: rec.base.package.clone(),
                app_label: app_label.clone(),
                key: key.clone(),
                title: title.clone(),
                is_group: rec.is_group,
                messages: Vec::new(),
                unread: 0,
            });
            let label_changed = conv.app_label != app_label;
            conv.app_label = app_label;
            if !title.is_empty() {
                conv.title = title;
            }
            conv.is_group |= rec.is_group;

            let mut added = 0u32;
            let oldest_retained = if conv.messages.len() >= MAX_MESSAGES_PER_CONVERSATION {
                conv.messages.first().map(|m| m.timestamp_ms)
            } else {
                None
            };

            for msg in incoming.drain(..) {
                if let Some(floor_ts) = oldest_retained {
                    if msg.timestamp_ms < floor_ts {
                        continue;
                    }
                }
                let duplicate = conv.messages.iter().any(|existing| {
                    existing.sender == msg.sender
                        && existing.text == msg.text
                        && existing.timestamp_ms == msg.timestamp_ms
                });
                if !duplicate {
                    conv.messages.push(msg);
                    added = added.saturating_add(1);
                }
            }

            if added > 0 {
                conv.messages.sort_by_key(|m| m.timestamp_ms);
                if conv.messages.len() > MAX_MESSAGES_PER_CONVERSATION {
                    let drop_n = conv.messages.len() - MAX_MESSAGES_PER_CONVERSATION;
                    conv.messages.drain(..drop_n);
                }
                conv.unread = conv.unread.saturating_add(added);
            }
            if (added > 0 || label_changed) && !changed.contains(&key) {
                changed.push(key);
            }
        }

        changed
    }

    pub fn conversations(&self, serial: &str) -> Vec<Conversation> {
        let Some(map) = self.by_serial.get(serial) else {
            return Vec::new();
        };
        let mut out: Vec<Conversation> = map.values().cloned().collect();
        out.sort_by(|a, b| {
            let ta = a.messages.last().map(|m| m.timestamp_ms).unwrap_or(0);
            let tb = b.messages.last().map(|m| m.timestamp_ms).unwrap_or(0);
            tb.cmp(&ta).then_with(|| a.title.cmp(&b.title))
        });
        out
    }

    pub fn mark_read(&mut self, serial: &str, key: &str) {
        if let Some(map) = self.by_serial.get_mut(serial) {
            if let Some(conv) = map.get_mut(key) {
                conv.unread = 0;
            }
        }
    }

    pub fn clear(&mut self, serial: &str) {
        self.by_serial.remove(serial);
    }

    pub fn clear_all(&mut self) {
        self.by_serial.clear();
    }

    pub(crate) fn refresh_app_labels(
        &mut self,
        serial: &str,
        labels: &HashMap<String, String>,
    ) -> Vec<String> {
        let Some(map) = self.by_serial.get_mut(serial) else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for conv in map.values_mut() {
            if let Some(label) = labels.get(&conv.package) {
                if conv.app_label != *label {
                    conv.app_label = label.clone();
                    changed.push(conv.key.clone());
                }
            }
        }
        changed
    }

    pub(crate) fn tracked_serials(&self) -> Vec<String> {
        self.by_serial.keys().cloned().collect()
    }
}

#[tauri::command]
pub fn phone_get_conversations(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
) -> Result<Vec<Conversation>, String> {
    validate_serial(&serial)?;
    let mut convs = {
        let store = state.messages.lock().map_err(crate::state::poison_error)?;
        store.conversations(&serial)
    };
    for conv in &mut convs {
        if conv.app_label.is_empty() || conv.app_label == conv.package {
            let resolved = resolve_app_label_cached(&app, &state, &serial, &conv.package, None);
            if resolved != conv.package {
                conv.app_label = resolved;
            }
        }
    }
    Ok(convs)
}

#[tauri::command]
pub fn phone_mark_conversation_read(
    state: State<'_, PhoneHubState>,
    serial: String,
    key: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let mut store = state.messages.lock().map_err(crate::state::poison_error)?;
    store.mark_read(&serial, &key);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone_files::parse_dumpsys_notifications_rich;

    #[test]
    fn dedupes_group_messages_with_unicode() {
        let raw = include_str!("../../tests/fixtures/phone_hub/notif_group_chat.txt");
        let records = parse_dumpsys_notifications_rich(raw);
        let mut store = MessageStore::default();
        let mut labels = HashMap::new();
        labels.insert("com.example.chat".to_string(), "Example Chat".to_string());

        let changed = store.ingest("SER1", &records, &labels);
        assert_eq!(changed.len(), 1);
        let convs = store.conversations("SER1");
        assert_eq!(convs.len(), 1);
        assert!(convs[0].is_group);
        assert_eq!(convs[0].title, "Weekend Crew");
        assert_eq!(convs[0].app_label, "Example Chat");
        assert_eq!(convs[0].messages.len(), 3);
        assert_eq!(convs[0].messages[0].sender, "Sam");
        assert_eq!(convs[0].messages[1].sender, "Sam");
        assert_eq!(convs[0].unread, 3);

        // Re-ingesting exact same snapshot must dedupe and report no changes.
        let changed2 = store.ingest("SER1", &records, &labels);
        assert!(changed2.is_empty());
        assert_eq!(store.conversations("SER1")[0].messages.len(), 3);
        assert_eq!(store.conversations("SER1")[0].unread, 3);

        store.mark_read("SER1", &convs[0].key);
        assert_eq!(store.conversations("SER1")[0].unread, 0);
    }

    #[test]
    fn retention_keeps_last_200_messages() {
        let mut store = MessageStore::default();
        let labels = HashMap::new();
        let mut msgs = Vec::new();
        for i in 0..250i64 {
            msgs.push(crate::phone_files::ParsedChatEntry {
                sender: "Alex".to_string(),
                text: format!("msg {i}"),
                timestamp_ms: 1_700_000_000_000 + i * 1000,
            });
        }
        let rec = ParsedNotificationRecord {
            base: crate::phone_files::PhoneNotification {
                key: "k1".into(),
                package: "com.example.chat".into(),
                app_label: Some("Chat".into()),
                title: "Alex".into(),
                text: "msg 249".into(),
                posted_unix: 1_700_000_249,
            },
            shortcut: Some("alex_1".into()),
            conversation_title: None,
            is_group: false,
            is_messaging: true,
            messages: msgs,
        };
        store.ingest("SER1", &[rec], &labels);
        let convs = store.conversations("SER1");
        assert_eq!(convs[0].messages.len(), 200);
        assert_eq!(convs[0].messages[0].text, "msg 50");
        assert_eq!(convs[0].messages[199].text, "msg 249");
    }

    #[test]
    fn conversations_are_per_serial_and_clear_empties() {
        let raw1 = include_str!("../../tests/fixtures/phone_hub/notif_messaging_style.txt");
        let raw2 = include_str!("../../tests/fixtures/phone_hub/notif_oem_variant.txt");
        let r1 = parse_dumpsys_notifications_rich(raw1);
        let r2 = parse_dumpsys_notifications_rich(raw2);

        let mut store = MessageStore::default();
        let labels = HashMap::new();
        store.ingest("SER_A", &r1, &labels);
        store.ingest("SER_B", &r2, &labels);

        assert_eq!(store.conversations("SER_A").len(), 1);
        assert_eq!(store.conversations("SER_A")[0].title, "Alex");
        assert_eq!(store.conversations("SER_B").len(), 1);
        assert_eq!(store.conversations("SER_B")[0].title, "Pat");

        store.clear("SER_A");
        assert!(store.conversations("SER_A").is_empty());
        assert_eq!(store.conversations("SER_B").len(), 1);
    }

    #[test]
    fn non_chat_notifications_ignored() {
        let raw = include_str!("../../tests/fixtures/phone_hub/notif_messaging_style.txt");
        let records = parse_dumpsys_notifications_rich(raw);
        let mut store = MessageStore::default();
        store.ingest("SER1", &records, &HashMap::new());
        let convs = store.conversations("SER1");
        // notif_messaging_style.txt has 1 chat notification and 1 Verification Code BigTextStyle.
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].package, "com.example.chat");
    }

    #[test]
    fn promo_notifications_ignored_by_message_store() {
        let raw = include_str!("../../tests/fixtures/phone_hub/notif_promo.txt");
        let records = parse_dumpsys_notifications_rich(raw);
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| !r.is_messaging));

        let mut store = MessageStore::default();
        let changed = store.ingest("SER1", &records, &HashMap::new());
        assert!(changed.is_empty());
        assert!(store.conversations("SER1").is_empty());
    }

    #[test]
    fn messaging_style_and_conversation_data_accepted() {
        let raw = include_str!("../../tests/fixtures/phone_hub/notif_messaging_style.txt");
        let records = parse_dumpsys_notifications_rich(raw);
        let mut store = MessageStore::default();
        let mut labels = HashMap::new();
        labels.insert("com.example.chat".to_string(), "Example Chat".to_string());
        store.ingest("SER1", &records, &labels);
        let convs = store.conversations("SER1");
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].app_label, "Example Chat");
        assert_eq!(convs[0].title, "Alex");

        // Also accept notifications carrying android.messagingStyleUser or category=msg + shortcut.
        let extra_raw = r#"Current Notification Manager state:
  Notification List:
    NotificationRecord(0x0777aaaa: pkg=com.example.chat user=UserHandle{0} id=70 tag=null importance=4 key=0|com.example.chat|70|null|10100: Notification(channel=dm shortcut=null flags=AUTO_CANCEL vis=PRIVATE))
      key=0|com.example.chat|70|null|10100
      notification=
            when=1787287600000/1787287600000
            extras={
                android.title=String (Morgan)
                android.text=String (See you at 5!)
                android.messagingStyleUser=Bundle (Bundle[mParcelledData.dataSize=64])
            }
    NotificationRecord(0x0888bbbb: pkg=com.example.sms user=UserHandle{0} id=80 tag=null importance=4 key=0|com.example.sms|80|null|10200: Notification(channel=sms shortcut=conv_lee flags=AUTO_CANCEL category=msg vis=PRIVATE))
      key=0|com.example.sms|80|null|10200
      notification=
            when=1787287700000/1787287700000
            extras={
                android.title=String (Lee)
                android.text=String (Running 5 mins late)
            }
"#;
        let extra_recs = parse_dumpsys_notifications_rich(extra_raw);
        assert_eq!(extra_recs.len(), 2);
        assert!(extra_recs.iter().all(|r| r.is_messaging));

        store.ingest("SER2", &extra_recs, &labels);
        assert_eq!(store.conversations("SER2").len(), 2);
    }

    #[test]
    fn label_refresh_updates_conversation_and_reports_changed() {
        let raw = include_str!("../../tests/fixtures/phone_hub/notif_messaging_style.txt");
        let records = parse_dumpsys_notifications_rich(raw);
        let mut store = MessageStore::default();
        store.ingest("SER1", &records, &HashMap::new());
        assert_eq!(store.conversations("SER1")[0].app_label, "Example Chat");

        // Simulate missing appName so package ID was initially used.
        let mut no_label_recs = records.clone();
        for rec in &mut no_label_recs {
            rec.base.app_label = None;
        }
        let mut store2 = MessageStore::default();
        store2.ingest("SER1", &no_label_recs, &HashMap::new());
        assert_eq!(
            store2.conversations("SER1")[0].app_label,
            "com.example.chat"
        );

        let mut labels = HashMap::new();
        labels.insert("com.example.chat".to_string(), "Example Chat".to_string());
        let changed = store2.refresh_app_labels("SER1", &labels);
        assert_eq!(changed.len(), 1);
        assert_eq!(store2.conversations("SER1")[0].app_label, "Example Chat");
    }
}
