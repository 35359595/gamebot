extern crate discord;
extern crate rand;
extern crate regex;
extern crate sqlite;

use discord::{
    model::{Event, ReactionEmoji},
    Discord,
};
use rand::{seq::SliceRandom, thread_rng};
use regex::Regex;
use sqlite::{Connection, Row};
use std::{env, fmt::Display};

struct Question {
    question: String,
    answer: String,
    score: i64,
    bold: Regex,
}

impl Question {
    fn new(question: String, answer: String, score: i64) -> Self {
        Question {
            question,
            answer,
            score,
            bold: Regex::new(r"(\[B\])|(\[\/B])").unwrap(),
        }
    }
}

impl Display for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{} ({} літер) [+{}]",
            self.bold.replace_all(&self.question, "**"),
            self.answer.chars().count(),
            self.score
        ))
    }
}

fn next_question(r: &Row) -> Question {
    let new_answer = r.read::<&str, _>("word").replace(|c: char| c == '\"', "");
    Question::new(
        r.read::<&str, _>("interpretation").to_string(),
        new_answer,
        r.read::<i64, _>("id_syn"),
    )
}

fn get_score(db: &Connection, user: u64) -> (i64, usize, usize) {
    let q = format!("SELECT * FROM scores WHERE user == {user}");
    if let Some(Ok(data)) = db.prepare(&q).unwrap().into_iter().last() {
        let score = data.read::<i64, _>("score");
        let mut all_scores = db
            .prepare("SELECT * FROM scores")
            .unwrap()
            .into_iter()
            .map(|r| r.unwrap().read::<i64, _>("score"))
            .collect::<Vec<i64>>();
        all_scores.sort();
        all_scores.reverse();
        let standing = all_scores.iter().position(|n| *n == score).unwrap() + 1;
        (score, standing, all_scores.len())
    } else {
        (0, 0, 0)
    }
}

fn get_top(db: &Connection) -> Vec<(i64, i64)> {
    let mut accum = Vec::with_capacity(10);
    db.prepare("SELECT * FROM scores ORDER BY score DESC LIMIT 10")
        .unwrap()
        .into_iter()
        .map(|r| {
            let row = r.unwrap();
            let score = row.read::<i64, _>("score");
            let user = row.read::<i64, _>("user");
            accum.push((user, score));
        })
        .for_each(drop);
    accum
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

fn produce_hint(q: &Question) -> String {
    let mut hint = q
        .answer
        .chars()
        .into_iter()
        .rev()
        .last()
        .unwrap()
        .to_string();
    hint.push_str(&str::repeat("◾", q.answer.chars().count() - 2));
    hint.push(q.answer.chars().last().unwrap());
    hint
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
        "SELECT * FROM wlist WHERE interpretation IS NOT NULL AND word NOT LIKE '(%)' AND word NOT LIKE 'Te саме%'";
    const SCORE_TABLE_CREATE: &str =
        "CREATE TABLE IF NOT EXISTS scores (user INTEGER PRIMARY KEY UNIQUE, score INTEGER)";
    let db = sqlite::open(&db_path).expect("db expected");
    // Create if not present `score` table
    let _ = db.execute(SCORE_TABLE_CREATE).unwrap();
    let mut data: Vec<Question> = db
        .prepare(QUERY)
        .unwrap()
        .into_iter()
        .map(|row| next_question(&row.unwrap()))
        .collect();
    println!("Loaded {} questions!", { data.len() });

    // RNG
    let mut rng = thread_rng();
    data.shuffle(&mut rng);

    // Log in to Discord using a bot token from the environment
    let discord = Discord::from_bot_token(&env::var("DISCORD_TOKEN").expect("Expected token"))
        .expect("login failed");

    // Establish and use a websocket connection
    let (mut connection, _) = discord.connect().expect("connect failed");
    println!("Ready.");
    let mut current_question = data.choose(&mut rng).expect("no more questions?");

    loop {
        match connection.recv_event() {
            Ok(Event::ReactionAdd(reaction))
                if reaction.emoji.eq(&ReactionEmoji::Unicode("❓".into())) =>
            {
                let target = discord
                    .get_message(reaction.channel_id, reaction.message_id)
                    .unwrap();
                if target.author.id.0 == 1165155849409405020 {
                    discord
                        .send_message(
                            reaction.channel_id,
                            &produce_hint(&current_question),
                            "",
                            false,
                        )
                        .unwrap();
                }
            }
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
                            &current_question.answer,
                            "",
                            false,
                        );
                        current_question = data.choose(&mut rng).expect("no more questions?");
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
                    } else if text == "!підказка" || text == "!хінт" {
                        let _ = discord.send_message(
                            message.channel_id,
                            &produce_hint(&current_question),
                            "",
                            false,
                        );
                    } else if text == "!рейтинг" {
                        let (score, standing, total) = get_score(&db, message.author.id.0);
                        if score == 0 {
                            let _ = discord.send_message(
                                message.channel_id,
                                &format!("{} нічого ще не відгадано...", message.author.mention()),
                                "",
                                false,
                            );
                        } else {
                            let _ = discord.send_message(
                                message.channel_id,
                                &format!(
                                    "{} має {} очок і є {} зі {}",
                                    message.author.mention(),
                                    score,
                                    standing,
                                    total
                                ),
                                "",
                                false,
                            );
                        }
                    } else if text == "!топ" {
                        let top = get_top(&db);
                        let mut top_report = String::default();
                        top.into_iter()
                            .enumerate()
                            .map(|(id, (user, score))| {
                                top_report.push_str(
                                    format!(
                                        "{}    |    {}    |    {}\n",
                                        id + 1,
                                        discord
                                            .get_user(discord::model::UserId(
                                                user.try_into().unwrap()
                                            ))
                                            .unwrap()
                                            .mention(),
                                        score,
                                    )
                                    .as_str(),
                                );
                            })
                            .for_each(drop);
                        let _ = discord.send_message(message.channel_id, &top_report, "", false);
                    } else if text == "!?" || text == "!help" {
                        let _ = discord.send_message(
                            message.channel_id,
                            "Відгадати слово за визначеням з тлумачного словника Української мови. Реєстр і навколишній текст не враховуються.\n\
Рейтинг [вказаний в квадратних дужках після кожного питання] додається гравцю за вірну відповідь і є вищий у рідше вживаних слів.\n\
**!?** | **!help** - інформація і команди;\n\
**!next** | **!далі** | **!відповідь** | ⏭️  - відповідь на поточне пиатння і нове питання;\n\
**!q** | **!питання** | **!п** - повторити поточне питання;\n\
**!підказка** | **!хінт** | реакція ❓ до питання - відобразити першу літеру відповіді;\n\
**!топ** - відобразити топ 10 гравців з найвищим рейтингом;\n\
**!рейтинг** - відобразити Ваш рейтинг;",
                            "",
                            false,
                        );
                    }
                } else if text.contains(&current_question.answer) {
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
                    current_question = data.choose(&mut rng).unwrap();
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

#[test]
fn bold_test() {
    let r = Regex::new(r"(\[B\])|(\[\/B])").unwrap();
    let res = r.replace_all("Те саме, що [B]заванта́жувати[/B]", "**");
    assert_eq!(res, "Те саме, що **заванта́жувати**")
}
