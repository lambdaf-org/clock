use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE, HiddenAct};
use hf_hub::{Repo, RepoType, api::sync::Api};
use std::collections::HashMap;
use tokenizers::Tokenizer;

const MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
const REVISION: &str = "refs/pr/21";

const TIER_THRESHOLDS: [(usize, i64); 6] = [
    (1, 0),
    (2, 1200),
    (3, 2400),
    (4, 3600),
    (5, 4500),
    (6, 5400),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    Architect,
    Visionary,
    Executor,
    Analyst,
    Ghost,
    Strategist,
    Maverick,
}

impl std::fmt::Display for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Style::Architect => write!(f, "architect"),
            Style::Visionary => write!(f, "visionary"),
            Style::Executor => write!(f, "executor"),
            Style::Analyst => write!(f, "analyst"),
            Style::Ghost => write!(f, "ghost"),
            Style::Strategist => write!(f, "strategist"),
            Style::Maverick => write!(f, "maverick"),
        }
    }
}

/// Long, rich descriptions for each style. These are what the model compares against.
/// Longer = better separation in embedding space.
fn style_descriptions() -> Vec<(Style, &'static str)> {
    vec![
        (
            Style::Architect,
            "A person who builds engines, bots, tools, platforms, provisioning systems, \
            collaboration tools, access management, real-time systems, database work, server code, \
            automation scripts, integration code, developer tooling, infrastructure code",
        ),
        (
            Style::Visionary,
            "A person who creates landing pages, designs products, builds brands, \
            launches new things, makes prototypes, builds user-facing applications, \
            grade management tools, visual dashboards, presentation materials",
        ),
        (
            Style::Executor,
            "A person who does physical manual labor, carries boxes, cleans floors, \
            washes dishes, mows lawns, paints walls, moves furniture, digs holes, \
            lifts heavy objects, sweeps, mops, scrubs, hauls trash, stacks shelves",
        ),
        (
            Style::Analyst,
            "A person who does research, studies neuroscience, cognitive science, \
            analyzes benchmarks, writes academic papers, diploma thesis, university coursework, \
            runs experiments, collects measurements, reads scientific papers",
        ),
        (
            Style::Ghost,
            "A person who works silently in the background, fixes bugs nobody notices, \
            maintains old code, does cleanup, handles invisible maintenance, \
            runs background scripts, monitors systems quietly",
        ),
        (
            Style::Strategist,
            "A person who plans projects, coordinates teams, manages roadmaps, \
            organizes schedules, delegates tasks, writes project plans, \
            tracks milestones, manages stakeholders, prioritizes tasks",
        ),
        (
            Style::Maverick,
            "A person who experiments with side projects, builds random things for fun, \
            explores new technologies, creates games, physics engines, visualizers, \
            hacks on hobby projects, tries new things, builds something unusual",
        ),
    ]
}

/// Word pool: style × tier → words
fn word_pool() -> HashMap<(Style, usize), Vec<&'static str>> {
    let mut pool = HashMap::new();

    // Architect
    pool.insert(
        (Style::Architect, 1),
        vec!["Sketcher", "Planner", "Draftsman", "Mapper", "Framer"],
    );
    pool.insert(
        (Style::Architect, 2),
        vec![
            "Builder",
            "Structurer",
            "Engineer",
            "Contractor",
            "Designer",
        ],
    );
    pool.insert(
        (Style::Architect, 3),
        vec!["Warden", "Steward", "Overseer", "Director", "Commander"],
    );
    pool.insert(
        (Style::Architect, 4),
        vec!["Rampart", "Bastion", "Pillar", "Fortress", "Ironclad"],
    );
    pool.insert(
        (Style::Architect, 5),
        vec![
            "Keystone",
            "Cornerstone",
            "Architect",
            "Sovereign",
            "Monument",
        ],
    );
    pool.insert(
        (Style::Architect, 6),
        vec!["Bedrock", "Foundation", "Monolith", "Colossus", "Obelisk"],
    );

    // Visionary
    pool.insert(
        (Style::Visionary, 1),
        vec!["Spark", "Dreamer", "Seeker", "Wanderer", "Explorer"],
    );
    pool.insert(
        (Style::Visionary, 2),
        vec![
            "Torchbearer",
            "Pathfinder",
            "Trailblazer",
            "Pioneer",
            "Vanguard",
        ],
    );
    pool.insert(
        (Style::Visionary, 3),
        vec!["Herald", "Beacon", "Firebrand", "Luminary", "Prophet"],
    );
    pool.insert(
        (Style::Visionary, 4),
        vec![
            "Catalyst",
            "Tempest",
            "Iconoclast",
            "Firestarter",
            "Harbinger",
        ],
    );
    pool.insert(
        (Style::Visionary, 5),
        vec!["Seer", "Mystic", "Visionary", "Phenomenon", "Revelation"],
    );
    pool.insert(
        (Style::Visionary, 6),
        vec![
            "Supernova",
            "Singularity",
            "Event Horizon",
            "Big Bang",
            "Legend",
        ],
    );

    // Executor
    pool.insert(
        (Style::Executor, 1),
        vec!["Grunt", "Soldier", "Worker", "Grinder", "Hustler"],
    );
    pool.insert(
        (Style::Executor, 2),
        vec!["Mule", "Bulldog", "Workhorse", "Ironside", "Tank"],
    );
    pool.insert(
        (Style::Executor, 3),
        vec![
            "Enforcer",
            "Crusher",
            "Berserker",
            "Steamroller",
            "Juggernaut",
        ],
    );
    pool.insert(
        (Style::Executor, 4),
        vec![
            "Demolisher",
            "Ravager",
            "Destroyer",
            "Obliterator",
            "Annihilator",
        ],
    );
    pool.insert(
        (Style::Executor, 5),
        vec!["Goliath", "Mammoth", "Titan", "Behemoth", "Leviathan"],
    );
    pool.insert(
        (Style::Executor, 6),
        vec![
            "Cataclysm",
            "Apocalypse",
            "Extinction",
            "Armageddon",
            "Ragnarok",
        ],
    );

    // Analyst
    pool.insert(
        (Style::Analyst, 1),
        vec!["Novice", "Listener", "Student", "Watcher", "Observer"],
    );
    pool.insert(
        (Style::Analyst, 2),
        vec![
            "Auditor",
            "Examiner",
            "Researcher",
            "Investigator",
            "Scholar",
        ],
    );
    pool.insert(
        (Style::Analyst, 3),
        vec![
            "Decoder",
            "Analyst",
            "Diagnostician",
            "Strategist",
            "Cryptographer",
        ],
    );
    pool.insert(
        (Style::Analyst, 4),
        vec!["Prodigy", "Savant", "Virtuoso", "Polymath", "Mastermind"],
    );
    pool.insert(
        (Style::Analyst, 5),
        vec![
            "Chronicler",
            "Sage",
            "Clairvoyant",
            "All-Seer",
            "Omniscient",
        ],
    );
    pool.insert(
        (Style::Analyst, 6),
        vec![
            "Black Box",
            "Zero Error",
            "Doomreader",
            "Absolute",
            "Final Answer",
        ],
    );

    // Ghost
    pool.insert(
        (Style::Ghost, 1),
        vec!["Drift", "Murmur", "Whisper", "Shade", "Shadow"],
    );
    pool.insert(
        (Style::Ghost, 2),
        vec!["Silhouette", "Ghost", "Specter", "Phantom", "Wraith"],
    );
    pool.insert(
        (Style::Ghost, 3),
        vec![
            "Haunt",
            "Nightcrawler",
            "Apparition",
            "Poltergeist",
            "Revenant",
        ],
    );
    pool.insert(
        (Style::Ghost, 4),
        vec!["Mirage", "Enigma", "Cipher", "Null", "Void"],
    );
    pool.insert(
        (Style::Ghost, 5),
        vec!["Limbo", "Eclipse", "Nether", "Abyss", "Oblivion"],
    );
    pool.insert(
        (Style::Ghost, 6),
        vec!["Erased", "Forgotten", "Nameless", "Nonexistent", "Nothing"],
    );

    // Strategist
    pool.insert(
        (Style::Strategist, 1),
        vec!["Spotter", "Lookout", "Watchman", "Sentinel", "Guard"],
    );
    pool.insert(
        (Style::Strategist, 2),
        vec!["Operator", "Handler", "Plotter", "Schemer", "Tactician"],
    );
    pool.insert(
        (Style::Strategist, 3),
        vec!["Marshal", "Warlord", "General", "Chancellor", "Kingmaker"],
    );
    pool.insert(
        (Style::Strategist, 4),
        vec!["Regent", "Dictator", "Tyrant", "Overlord", "Emperor"],
    );
    pool.insert(
        (Style::Strategist, 5),
        vec![
            "Phantom King",
            "Eminence",
            "Grandmaster",
            "Chessmaster",
            "Puppetmaster",
        ],
    );
    pool.insert(
        (Style::Strategist, 6),
        vec!["Endgame", "Omega", "Unkillable", "Inevitable", "Checkmate"],
    );

    // Maverick
    pool.insert(
        (Style::Maverick, 1),
        vec!["Stray", "Rookie", "Drifter", "Wildcard", "Rebel"],
    );
    pool.insert(
        (Style::Maverick, 2),
        vec!["Rogue", "Bandit", "Outlaw", "Maverick", "Renegade"],
    );
    pool.insert(
        (Style::Maverick, 3),
        vec![
            "Gunslinger",
            "Corsair",
            "Vigilante",
            "Mercenary",
            "Desperado",
        ],
    );
    pool.insert(
        (Style::Maverick, 4),
        vec!["Exile", "Heretic", "Usurper", "Kingslayer", "Pirate King"],
    );
    pool.insert(
        (Style::Maverick, 5),
        vec!["Outcast", "Boogeyman", "Nightmare", "Folklore", "Myth"],
    );
    pool.insert(
        (Style::Maverick, 6),
        vec![
            "Unbound",
            "Unchained",
            "Impossible",
            "Untouchable",
            "Anomaly",
        ],
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
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub struct RoleClassifier {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    style_embeddings: Vec<StyleEmbedding>,
}

pub struct StyleEmbedding {
    style: Style,
    embedding: Vec<f32>,
}

impl RoleClassifier {
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
            style_embeddings: Vec::new(),
        };

        // Embed the 7 style descriptions
        println!("[roles] Embedding style descriptions...");
        for (style, desc) in style_descriptions() {
            let embedding = classifier.embed(desc)?;
            classifier
                .style_embeddings
                .push(StyleEmbedding { style, embedding });
        }
        println!(
            "[roles] Ready. {} styles embedded.",
            classifier.style_embeddings.len()
        );

        Ok(classifier)
    }

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

        let (_n_sentence, n_tokens, _hidden_size) = output.dims3()?;
        let mean = (output.sum(1)? / (n_tokens as f64))?;

        let raw: Vec<f32> = mean.squeeze(0)?.to_vec1()?;
        Ok(normalize_l2(&raw))
    }

    /// Classify: embed activities → match against 7 styles → pick word from winning style+tier
    pub fn classify(
        &self,
        activities: &[(String, i64)],
        total_minutes: i64,
    ) -> anyhow::Result<(String, String, usize)> {
        let tier = minutes_to_tier(total_minutes);

        // Score each style
        let mut style_scores: HashMap<Style, f32> = HashMap::new();
        let mut total_weight = 0i64;

        for (activity, minutes) in activities {
            // Pure "work" has zero semantic signal — skip it entirely
            if activity == "work" {
                continue;
            }

            let mut clean = activity.replace('-', " ");
            // Strip "work" from anywhere — it adds no semantic signal
            clean = clean.replace("work", "").trim().to_string();
            // Collapse multiple spaces
            while clean.contains("  ") {
                clean = clean.replace("  ", " ");
            }
            if clean.is_empty() {
                continue;
            }

            println!("[roles]   activity: '{}' ({}min)", clean, minutes);

            let activity_emb = self.embed(&clean)?;

            for se in &self.style_embeddings {
                let sim = cosine_similarity(&activity_emb, &se.embedding);
                *style_scores.entry(se.style).or_insert(0.0) += sim * (*minutes as f32);
            }
            total_weight += minutes;
        }

        // Normalize by total weight
        // If no activities had signal (all "work"), default to executor
        if total_weight == 0 {
            let pool = word_pool();
            let words = pool
                .get(&(Style::Executor, tier))
                .map(|v| v.as_slice())
                .unwrap_or(&["Unknown"]);
            let idx = (total_minutes as usize) % words.len();
            let word = words[idx];
            let role = format_role(tier, word);
            println!("[roles] No signal, defaulting to executor → {}", role);
            return Ok((role, word.to_string(), tier));
        }

        for score in style_scores.values_mut() {
            *score /= total_weight as f32;
        }

        // Sort styles by score
        let mut sorted: Vec<(Style, f32)> = style_scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Debug logging
        println!("[roles] Tier {} style scores:", tier);
        for (style, score) in &sorted {
            println!("[roles]   {:.4} {}", score, style);
        }

        let winning_style = sorted.first().map(|(s, _)| *s).unwrap_or(Style::Executor);

        // Pick a word from the winning style+tier cell
        let pool = word_pool();
        let words = pool
            .get(&(winning_style, tier))
            .map(|v| v.as_slice())
            .unwrap_or(&["Unknown"]);

        // Sub-rank: position within the tier determines which word (ascending)
        let tier_starts: [i64; 7] = [0, 0, 1200, 2400, 3600, 4500, 5400];
        let tier_ends: [i64; 7] = [0, 1200, 2400, 3600, 4500, 5400, 7200];
        let start = tier_starts[tier];
        let end = tier_ends[tier];
        let range = (end - start).max(1);
        let position = ((total_minutes - start) as f64) / (range as f64);
        let idx = ((position * words.len() as f64) as usize).min(words.len() - 1);
        let word = words[idx];

        let role = format_role(tier, word);
        println!("[roles] Winner: {} → {}", winning_style, role);

        Ok((role, word.to_string(), tier))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minutes_to_tier() {
        assert_eq!(minutes_to_tier(0), 1);
        assert_eq!(minutes_to_tier(1200), 2);
        assert_eq!(minutes_to_tier(2940), 3);
        assert_eq!(minutes_to_tier(3600), 4);
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
        assert_eq!(pool.len(), 42); // 7 styles × 6 tiers
        for (_, words) in &pool {
            assert_eq!(words.len(), 5);
        }
    }
}
