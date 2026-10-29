use serde::{Deserialize, Serialize};

use crate::types::{Ack, Fill, Market, Matcher, Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vector {
    pub name: String,
    pub market: Market,
    pub input: Vec<Message>,
    pub expected: Vec<ExpectedLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExpectedLine {
    Ack {
        ack: Ack,
    },
    Fill {
        fill: Fill,
    },
}

/// Run `input` through `make(market)` and compare output to `vector.expected`.
pub fn run_vector<M, F>(make: F, vector: &Vector) -> Result<(), String>
where
    M: Matcher,
    F: Fn(Market) -> M,
{
    let mut matcher = make(vector.market.clone());
    let mut actual: Vec<ExpectedLine> = Vec::new();

    for msg in &vector.input {
        let (ack, fills) = matcher.process(msg.clone());
        actual.push(ExpectedLine::Ack { ack });
        for fill in fills {
            actual.push(ExpectedLine::Fill { fill });
        }
    }

    if actual.len() != vector.expected.len() {
        return Err(format!(
            "{}: expected {} output lines, got {}",
            vector.name,
            vector.expected.len(),
            actual.len()
        ));
    }

    for (i, (got, want)) in actual.iter().zip(vector.expected.iter()).enumerate() {
        if got != want {
            let got_json = match serde_json::to_string(got) {
                Ok(s) => s,
                Err(e) => format!("<serialize error: {e}>"),
            };
            let want_json = match serde_json::to_string(want) {
                Ok(s) => s,
                Err(e) => format!("<serialize error: {e}>"),
            };
            return Err(format!(
                "{}: first mismatch at line {}: got {} expected {}",
                vector.name, i, got_json, want_json
            ));
        }
    }

    Ok(())
}
