use super::{json, Path};
use base64::{engine::general_purpose::STANDARD, Engine as _};

pub(super) const ROOT_TEXT: &str = "共享问题 来源 🚀";
pub(super) const CHOSEN_TEXT: &str = "ALTERNATE_BRANCH_SELECTED";
pub(super) const ANSWER: &str = "ALTERNATE_BRANCH_ANSWER";
pub(super) const SIBLING_TEXT: &str = "SIBLING_BRANCH_EXCLUDED";
pub(super) const SIBLING_ANSWER: &str = "SIBLING_ANSWER_EXCLUDED";
pub(super) const ASSET_IDS: [&str; 4] = [
    "chosen:img:1",
    "chosen:img:2",
    "chosen:img:3",
    "sibling:img:1",
];

pub(super) struct Source {
    pub(super) jsonl: Vec<u8>,
    pub(super) png: Vec<u8>,
    pub(super) gif: Vec<u8>,
}

impl Source {
    pub(super) fn create(root: &Path) -> Self {
        let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aCM0AAAAASUVORK5CYII=").unwrap();
        let gif = STANDARD
            .decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
            .unwrap();
        std::fs::create_dir_all(root).unwrap();
        // The denied IDs intentionally share bytes/digest with an allowed ID.
        // Authorization must bind the asset ID, not just the stored blob hash.
        for name in ["allowed.png", "excluded.png", "sibling.png"] {
            std::fs::write(root.join(name), &png).unwrap();
        }
        let records = [
            json!({"type":"user","uuid":"root","parentUuid":null,
                "sessionId":"branch-assets","message":{"content":ROOT_TEXT}}),
            json!({"type":"user","uuid":"chosen","parentUuid":"root",
            "sessionId":"branch-assets","message":{"content":[
                {"type":"text","text":CHOSEN_TEXT},
                {"type":"image","source":{"path":"allowed.png","media_type":"image/png"}},
                {"type":"image","source":{"data":STANDARD.encode(&gif),"media_type":"image/gif"}},
                {"type":"image","source":{"path":"excluded.png","media_type":"image/png"}}
            ]}}),
            json!({"type":"assistant","uuid":"chosen-answer","parentUuid":"chosen",
                "sessionId":"branch-assets","message":{"content":ANSWER}}),
            json!({"type":"user","uuid":"sibling","parentUuid":"root",
            "sessionId":"branch-assets","message":{"content":[
                {"type":"text","text":SIBLING_TEXT},
                {"type":"image","source":{"path":"sibling.png","media_type":"image/png"}}
            ]}}),
            json!({"type":"assistant","uuid":"sibling-answer","parentUuid":"sibling",
                "sessionId":"branch-assets","message":{"content":SIBLING_ANSWER}}),
        ];
        let jsonl = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>()
            .into_bytes();
        std::fs::write(root.join("branch-assets.jsonl"), &jsonl).unwrap();
        Self { jsonl, png, gif }
    }

    pub(super) fn unchanged(&self, root: &Path) {
        assert_eq!(
            std::fs::read(root.join("branch-assets.jsonl")).unwrap(),
            self.jsonl
        );
        for name in ["allowed.png", "excluded.png", "sibling.png"] {
            assert_eq!(std::fs::read(root.join(name)).unwrap(), self.png);
        }
    }
}
