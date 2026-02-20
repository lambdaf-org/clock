use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE, HiddenAct};
use hf_hub::{Repo, RepoType, api::sync::Api};
use std::collections::HashMap;
use tokenizers::Tokenizer;

const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
const REVISION: &str = "refs/pr/21";

/// Tier thresholds in minutes (weekly)
const TIER_THRESHOLDS: [(usize, i64); 6] = [
    (1, 0),    // 0-20h
    (2, 1200), // 20h
    (3, 2400), // 40h
    (4, 3600), // 60h
    (5, 4500), // 75h
    (6, 5400), // 90h
];

struct WordEntry {
    word: String,
    #[allow(dead_code)]
    style: String,
    tier: usize,
    embedding: Vec<f32>,
}

pub struct RoleClassifier {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    words: Vec<WordEntry>,
}

fn word_pool() -> HashMap<String, HashMap<usize, Vec<String>>> {
    let mut pool: HashMap<String, HashMap<usize, Vec<String>>> = HashMap::new();

    macro_rules! style {
        ($name:expr, $t1:expr, $t2:expr, $t3:expr, $t4:expr, $t5:expr, $t6:expr) => {{
            let mut tiers = HashMap::new();
            tiers.insert(1, $t1.iter().map(|s: &&str| s.to_string()).collect());
            tiers.insert(2, $t2.iter().map(|s: &&str| s.to_string()).collect());
            tiers.insert(3, $t3.iter().map(|s: &&str| s.to_string()).collect());
            tiers.insert(4, $t4.iter().map(|s: &&str| s.to_string()).collect());
            tiers.insert(5, $t5.iter().map(|s: &&str| s.to_string()).collect());
            tiers.insert(6, $t6.iter().map(|s: &&str| s.to_string()).collect());
            pool.insert($name.to_string(), tiers);
        }};
    }

    style!(
        "architect",
        &["Planner", "Draftsman", "Mapper", "Framer", "Sketcher"],
        &[
            "Engineer",
            "Designer",
            "Builder",
            "Structurer",
            "Contractor"
        ],
        &["Commander", "Warden", "Overseer", "Director", "Steward"],
        &["Ironclad", "Pillar", "Bastion", "Fortress", "Rampart"],
        &[
            "Sovereign",
            "Architect",
            "Cornerstone",
            "Keystone",
            "Monument"
        ],
        &["Colossus", "Monolith", "Foundation", "Bedrock", "Obelisk"]
    );
    style!(
        "visionary",
        &["Dreamer", "Seeker", "Wanderer", "Explorer", "Spark"],
        &[
            "Pioneer",
            "Trailblazer",
            "Pathfinder",
            "Torchbearer",
            "Vanguard"
        ],
        &["Prophet", "Beacon", "Luminary", "Herald", "Firebrand"],
        &[
            "Catalyst",
            "Harbinger",
            "Iconoclast",
            "Firestarter",
            "Tempest"
        ],
        &["Visionary", "Phenomenon", "Seer", "Mystic", "Revelation"],
        &[
            "Supernova",
            "Singularity",
            "Event Horizon",
            "Big Bang",
            "Legend"
        ]
    );
    style!(
        "executor",
        &["Worker", "Grinder", "Hustler", "Soldier", "Grunt"],
        &["Hammer", "Brute", "Workhorse", "Bulldog", "Mule"],
        &[
            "Juggernaut",
            "Steamroller",
            "Crusher",
            "Berserker",
            "Enforcer"
        ],
        &[
            "Destroyer",
            "Ravager",
            "Obliterator",
            "Demolisher",
            "Annihilator"
        ],
        &["Leviathan", "Behemoth", "Goliath", "Titan", "Mammoth"],
        &[
            "Apocalypse",
            "Cataclysm",
            "Extinction",
            "Armageddon",
            "Ragnarok"
        ]
    );
    style!(
        "analyst",
        &["Observer", "Watcher", "Student", "Listener", "Novice"],
        &[
            "Scholar",
            "Researcher",
            "Examiner",
            "Investigator",
            "Auditor"
        ],
        &[
            "Strategist",
            "Decoder",
            "Cryptographer",
            "Analyst",
            "Diagnostician"
        ],
        &["Mastermind", "Savant", "Prodigy", "Virtuoso", "Polymath"],
        &[
            "Omniscient",
            "All-Seer",
            "Clairvoyant",
            "Sage",
            "Chronicler"
        ],
        &[
            "Doomreader",
            "Final Answer",
            "Black Box",
            "Zero Error",
            "Absolute"
        ]
    );
    style!(
        "ghost",
        &["Shadow", "Whisper", "Shade", "Murmur", "Drift"],
        &["Phantom", "Specter", "Wraith", "Ghost", "Silhouette"],
        &[
            "Apparition",
            "Revenant",
            "Poltergeist",
            "Nightcrawler",
            "Haunt"
        ],
        &["Cipher", "Void", "Null", "Enigma", "Mirage"],
        &["Oblivion", "Abyss", "Nether", "Eclipse", "Limbo"],
        &["Nonexistent", "Forgotten", "Erased", "Nameless", "Nothing"]
    );
    style!(
        "strategist",
        &["Lookout", "Sentinel", "Spotter", "Watchman", "Guard"],
        &["Tactician", "Schemer", "Plotter", "Operator", "Handler"],
        &["General", "Chancellor", "Marshal", "Warlord", "Kingmaker"],
        &["Emperor", "Overlord", "Tyrant", "Dictator", "Regent"],
        &[
            "Puppetmaster",
            "Chessmaster",
            "Grandmaster",
            "Phantom King",
            "Eminence"
        ],
        &["Inevitable", "Unkillable", "Endgame", "Omega", "Checkmate"]
    );
    style!(
        "maverick",
        &["Rookie", "Rebel", "Stray", "Drifter", "Wildcard"],
        &["Renegade", "Outlaw", "Bandit", "Rogue", "Maverick"],
        &[
            "Mercenary",
            "Desperado",
            "Vigilante",
            "Gunslinger",
            "Corsair"
        ],
        &["Pirate King", "Kingslayer", "Usurper", "Heretic", "Exile"],
        &["Myth", "Folklore", "Nightmare", "Boogeyman", "Outcast"],
        &[
            "Unchained",
            "Unbound",
            "Untouchable",
            "Impossible",
            "Anomaly"
        ]
    );

    pool
}

pub fn minutes_to_tier(minutes: i64) -> usize {
    let mut tier = 1;
    for &(t, threshold) in &TIER_THRESHOLDS {
        if minutes >= threshold {
            tier = t;
        }
    }
    tier
}

pub fn format_role(tier: usize, word: &str) -> String {
    let roman = match tier {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        6 => "VI",
        _ => "?",
    };
    format!("⚡ [{}] {}", roman, word)
}

fn normalize_l2(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    // Both vectors are L2-normalized, so dot product = cosine similarity
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

impl RoleClassifier {
    /// Load the model and pre-embed all 210 words. Call once at startup.
    pub fn new() -> anyhow::Result<Self> {
        println!("[roles] Downloading model...");
        let api = Api::new()?;
        let repo = api.repo(Repo::with_revision(
            MODEL_ID.to_string(),
            RepoType::Model,
            REVISION.to_string(),
        ));

        let config_path = repo.get("config.json")?;
        let tokenizer_path = repo.get("tokenizer.json")?;
        let weights_path = repo.get("model.safetensors")?;

        println!("[roles] Loading model...");
        let device = Device::Cpu;

        let config_str = std::fs::read_to_string(&config_path)?;
        let mut config: Config = serde_json::from_str(&config_str)?;
        config.hidden_act = HiddenAct::Gelu;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer error: {}", e))?;

        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)? };
        let model = BertModel::load(vb, &config)?;

        let mut classifier = Self {
            model,
            tokenizer,
            device,
            words: Vec::with_capacity(210),
        };

        println!("[roles] Embedding word pool...");
        let pool = word_pool();
        let style_contexts: HashMap<String, &str> = HashMap::from([
            (
                "architect".into(),
                "building systems, infrastructure, provisioning, tooling, engines, frameworks, backends, devops, CI/CD",
            ),
            (
                "visionary".into(),
                "creating landing pages, designing products, pitching ideas, launching startups, branding, marketing",
            ),
            (
                "executor".into(),
                "work, grinding, getting things done, general tasks, admin, paperwork, operations, shipping",
            ),
            (
                "analyst".into(),
                "research, benchmarks, data analysis, cognitive science, papers, studying, neuroscience, statistics",
            ),
            (
                "ghost".into(),
                "background maintenance, silent fixes, invisible work, cleanup, nobody notices",
            ),
            (
                "strategist".into(),
                "planning, coordinating, managing projects, roadmaps, team strategy, scheduling, delegation",
            ),
            (
                "maverick".into(),
                "experimenting, side projects, random exploration, physics engines, visualizers, hobby projects, games",
            ),
        ]);
        for (style, tiers) in &pool {
            let ctx = style_contexts.get(style).unwrap_or(&"general work");
            for (&tier, words) in tiers {
                for word in words {
                    let context = format!("{} — {}", word, ctx);
                    let embedding = classifier.embed(&context)?;
                    classifier.words.push(WordEntry {
                        word: word.clone(),
                        style: style.clone(),
                        tier,
                        embedding,
                    });
                }
            }
        }
        println!("[roles] Ready. {} words embedded.", classifier.words.len());

        Ok(classifier)
    }

    /// Embed a text string → L2-normalized 384-dim vector.
    /// Matches candle's official BERT example exactly:
    ///   forward(&token_ids, &token_type_ids) → mean pool over tokens → L2 normalize
    fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize error: {}", e))?;

        let ids = encoding.get_ids().to_vec();
        let type_ids = encoding.get_type_ids().to_vec();

        let token_ids = Tensor::new(ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::new(type_ids.as_slice(), &self.device)?.unsqueeze(0)?;

        let output = self.model.forward(&token_ids, &token_type_ids, None)?;

        // Mean pooling (same as candle example: sum(1) / n_tokens)
        let (_n_sentence, n_tokens, _hidden_size) = output.dims3()?;
        let mean = (output.sum(1)? / (n_tokens as f64))?;

        let raw: Vec<f32> = mean.squeeze(0)?.to_vec1()?;
        Ok(normalize_l2(&raw))
    }

    /// Classify activities into the best word for this user's tier.
    pub fn classify(
        &self,
        activities: &[(String, i64)],
        total_minutes: i64,
    ) -> anyhow::Result<(String, String, usize)> {
        let tier = minutes_to_tier(total_minutes);

        let tier_words: Vec<&WordEntry> = self.words.iter().filter(|w| w.tier == tier).collect();
        if tier_words.is_empty() {
            return Ok((format_role(tier, "Unknown"), "Unknown".to_string(), tier));
        }

        let mut word_scores: Vec<(f32, &WordEntry)> = Vec::new();

        for word_entry in &tier_words {
            let mut weighted_score = 0.0f32;
            let mut total_weight = 0i64;

            for (activity, minutes) in activities {
                let activity_embedding = self.embed(activity)?;
                let sim = cosine_similarity(&activity_embedding, &word_entry.embedding);
                weighted_score += sim * (*minutes as f32);
                total_weight += minutes;
            }

            if total_weight > 0 {
                weighted_score /= total_weight as f32;
            }

            word_scores.push((weighted_score, word_entry));
        }

        word_scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let best = word_scores[0].1;
        Ok((format_role(tier, &best.word), best.word.clone(), tier))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minutes_to_tier() {
        assert_eq!(minutes_to_tier(0), 1);
        assert_eq!(minutes_to_tier(600), 1);
        assert_eq!(minutes_to_tier(1200), 2);
        assert_eq!(minutes_to_tier(2940), 3);
        assert_eq!(minutes_to_tier(3600), 4);
        assert_eq!(minutes_to_tier(4500), 5);
        assert_eq!(minutes_to_tier(5400), 6);
    }

    #[test]
    fn test_format_role() {
        assert_eq!(format_role(3, "Commander"), "⚡ [III] Commander");
        assert_eq!(format_role(6, "Ragnarok"), "⚡ [VI] Ragnarok");
    }

    #[test]
    fn test_word_pool_counts() {
        let pool = word_pool();
        assert_eq!(pool.len(), 7);
        let total: usize = pool
            .values()
            .flat_map(|t| t.values())
            .map(|w| w.len())
            .sum();
        assert_eq!(total, 210);
    }
}
