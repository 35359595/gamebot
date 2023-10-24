extern crate discord;
extern crate rand;
extern crate sqlite;

use discord::{
    model::{Event, ReactionEmoji},
    Discord,
};
use rand::{prelude::*, thread_rng};
use sqlite::{Connection, Row};
use std::{env, fmt::Display};

struct Question {
    question: String,
    answer: String,
    score: i64,
}

impl Question {
    fn new(question: String, answer: String, score: i64) -> Self {
        Question {
            question,
            answer,
            score,
        }
    }
}

impl Display for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{} ({} літер) [+{}]",
            self.question,
            self.answer.len(),
            self.score
        ))
    }
}

fn next_question(r: Row) -> Question {
    let new_answer = r.read::<&str, _>("word").replace(|c: char| c == '\"', "");
    println!("{new_answer}");
    Question::new(
        r.read::<&str, _>("interpretation").to_string(),
        new_answer,
        r.read::<i64, _>("id_syn"),
    )
}

fn increment_score(db: &Connection, user: u64, score: i64) -> i64 {
    let current = format!("SELECT * FROM scores WHERE user == {user}");
    let ignore_if_exist = format!("INSERT OR IGNORE INTO scores (user, score) VALUES ({user}, 0)");
    db.execute(ignore_if_exist).unwrap();
    let data = db
        .prepare(&current)
        .unwrap()
        .into_iter()
        .last()
        .unwrap()
        .unwrap();
    let total_score = data.read::<i64, _>("score") + score;
    let insert = format!("INSERT OR REPLACE INTO scores VALUES ({user}, {total_score})");
    let _ = db.execute(&insert).unwrap();
    total_score
}

fn main() {
    println!("Opening DB");
    let mut db_path = env::var("CARGO_MANIFEST_DIR").unwrap_or(
        env::current_dir()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap_or(".".into()),
    );
    db_path.push_str("/db/synsets_ua.db");
    println!("DB path: {}", &db_path);

    // Verb selector
    const QUERY: &str =
        "SELECT * FROM wlist WHERE interpretation IS NOT NULL AND word NOT LIKE '(%)'";
    const SCORE_TABLE_CREATE: &str =
        "CREATE TABLE IF NOT EXISTS scores (user INTEGER PRIMARY KEY UNIQUE, score INTEGER)";
    let db = sqlite::open(&db_path).expect("db expected");
    // Create if not present `score` table
    let _ = db.execute(SCORE_TABLE_CREATE).unwrap();
    let mut data: Vec<Row> = db
        .prepare(QUERY)
        .unwrap()
        .into_iter()
        .map(|row| row.unwrap())
        .collect();

    // RNG
    let mut rng = thread_rng();
    data.shuffle(&mut rng);

    // Log in to Discord using a bot token from the environment
    let discord = Discord::from_bot_token(&env::var("DISCORD_TOKEN").expect("Expected token"))
        .expect("login failed");

    // Establish and use a websocket connection
    let (mut connection, _) = discord.connect().expect("connect failed");
    println!("Ready.");
    let mut current_question = next_question(data.pop().unwrap());

    loop {
        match connection.recv_event() {
            Ok(Event::MessageCreate(message)) => {
                let text = message.content.to_owned().trim().to_lowercase();
                println!(
                    "{}: {} says: {}",
                    message.timestamp, message.author.name, text
                );
                // service commands
                if text.chars().rev().last().is_some_and(|c| c.eq(&'!')) {
                    if text == "!next" || text == "!далі" || text == "!відповідь" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.to_string(),
                            "",
                            false,
                        );
                        current_question = next_question(data.pop().unwrap());
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.to_string(),
                            "",
                            false,
                        );
                    } else if text == "!q" || text == "!питання" || text == "!п" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question.to_string(),
                            "",
                            false,
                        );
                    } else if text.trim().to_lowercase() == "!підказка" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &current_question
                                .answer
                                .chars()
                                .into_iter()
                                .rev()
                                .last()
                                .unwrap()
                                .to_string(),
                            "",
                            false,
                        );
                    }
                } else if text == current_question.answer {
                    // ansver verify and update score
                    let new_score =
                        increment_score(&db, message.author.id.0, current_question.score);
                    let _ = discord.send_message(
                        message.channel_id,
                        format!(
                            "Вірно {}. Відповідь {}. Загальний рейтинг: {}",
                            message.author.mention(),
                            current_question.answer,
                            new_score
                        )
                        .as_str(),
                        "",
                        false,
                    );
                    current_question = next_question(data.pop().unwrap());
                    let _ = discord.send_message(
                        message.channel_id,
                        &current_question.to_string(),
                        "",
                        false,
                    );
                } else if !message.author.bot {
                    let _ = discord.add_reaction(
                        message.channel_id,
                        message.id,
                        ReactionEmoji::Unicode("➖".to_string()),
                    );
                }
            }
            Ok(_) => {}
            Err(discord::Error::Closed(code, body)) => {
                println!("Gateway closed on us with code {:?}: {}", code, body);
                break;
            }
            Err(err) => println!("Receive error: {:?}", err),
        }
    }
}
