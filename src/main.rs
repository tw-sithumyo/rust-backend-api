use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::ErrorKind, str::FromStr, sync::Arc};
use uuid::Uuid;
use warp::{
    body::BodyDeserializeError,
    cors::CorsForbidden,
    hyper::{Method, StatusCode},
    reject::Reject,
    Filter, Rejection, Reply,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Question {
    id: QuestionId,
    title: String,
    content: String,
    faq: Option<Vec<String>>,
}

#[derive(Debug, Hash, PartialEq, Eq, Serialize, Deserialize, Clone)]
struct QuestionId(String);

impl FromStr for QuestionId {
    type Err = std::io::Error;
    fn from_str(id: &str) -> Result<Self, Self::Err> {
        match id.is_empty() {
            false => Ok(QuestionId(id.to_string())),
            true => Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "No id provided",
            )),
        }
    }
}

impl std::fmt::Display for QuestionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for Question {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ID: {}, Title: {}, Content: {}, FAQ: {:?}",
            self.id, self.title, self.content, self.faq
        )
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Answer {
    id: String,
    content: String,
    question_id: String,
}

#[derive(Clone, Debug)]
struct Store {
    questions: Arc<RwLock<HashMap<QuestionId, Question>>>,
    answers: Arc<RwLock<HashMap<String, Answer>>>,
}

impl Store {
    fn new() -> Self {
        Store {
            questions: Arc::new(RwLock::new(Self::init())),
            answers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn init() -> HashMap<QuestionId, Question> {
        let file = include_str!("../questions.json");
        serde_json::from_str(file).expect("Can't read questions.json")
    }
}

#[derive(Debug, PartialEq)]
enum ParameterError {
    ParseError(std::num::ParseIntError),
    MissingParameters(String),
    QuestionNotFound,
    OutOfIndex,
}
impl std::fmt::Display for ParameterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterError::ParseError(ref err) => write!(f, "Cannot parse parameter: {}", err),
            ParameterError::MissingParameters(ref parameter_name) => {
                write!(f, "Missing '{}' Parameter", parameter_name)
            }
            ParameterError::OutOfIndex => write!(f, "Out of Index Paramter"),
            ParameterError::QuestionNotFound => write!(f, "Question Not Found!"),
        }
    }
}
impl Reject for ParameterError {}

#[derive(Debug)]
struct Pagination {
    start: usize,
    end: usize,
}

fn extract_pagination(params: HashMap<String, String>) -> Result<Pagination, ParameterError> {
    if !params.contains_key("start") {
        return Err(ParameterError::MissingParameters("start".to_string()));
    }

    if !params.contains_key("end") {
        return Err(ParameterError::MissingParameters("end".to_string()));
    }

    return Ok(Pagination {
        start: params
            .get("start")
            .unwrap()
            .parse::<usize>()
            .map_err(ParameterError::ParseError)?,
        end: params
            .get("end")
            .unwrap()
            .parse::<usize>()
            .map_err(ParameterError::ParseError)?,
    });
}

async fn get_questions(
    params: HashMap<String, String>,
    store: Store,
) -> Result<impl Reply, Rejection> {
    if params.len() > 0 {
        let pagination = extract_pagination(params)?;
        let res: Vec<Question> = store.questions.read().values().cloned().collect();
        if let Some(res) = &res.get(pagination.start..pagination.end) {
            Ok(warp::reply::json(&res))
        } else {
            Err(ParameterError::OutOfIndex)?
        }
    } else {
        let res: Vec<Question> = store.questions.read().values().cloned().collect();
        Ok(warp::reply::json(&res))
    }
}

async fn get_single_question(
    id: String,
    store: Store,
) -> Result<impl warp::Reply, warp::Rejection> {
    match store.questions.read().get(&QuestionId(id)) {
        Some(q) => Ok(warp::reply::json(&q)),
        None => return Err(warp::reject::custom(ParameterError::QuestionNotFound)),
    }
}

async fn add_question(
    store: Store,
    question: Question,
) -> Result<impl warp::Reply, warp::Rejection> {
    store
        .questions
        .write()
        .insert(question.clone().id, question);
    Ok(warp::reply::with_status("Question Added", StatusCode::OK))
}

async fn update_question(
    id: String,
    store: Store,
    question: Question,
) -> Result<impl warp::Reply, warp::Rejection> {
    match store.questions.write().get_mut(&QuestionId(id)) {
        Some(q) => *q = question,
        None => return Err(warp::reject::custom(ParameterError::QuestionNotFound)),
    }
    Ok(warp::reply::with_status("Question Updated", StatusCode::OK))
}

async fn delete_question(id: String, store: Store) -> Result<impl warp::Reply, warp::Rejection> {
    match store.questions.write().remove(&QuestionId(id)) {
        Some(_) => return Ok(warp::reply::with_status("Question deleted", StatusCode::OK)),
        None => return Err(warp::reject::custom(ParameterError::QuestionNotFound)),
    }
}

async fn add_answer(
    store: Store,
    params: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
    if params.get("content") == None {
        return Err(warp::reject::custom(ParameterError::MissingParameters(
            "content".to_string(),
        )));
    }
    if params.get("relationId") == None {
        return Err(warp::reject::custom(ParameterError::MissingParameters(
            "relationId".to_string(),
        )));
    }
    let content = params.get("content").unwrap().to_string();
    let relation_id = params.get("relationId").unwrap().to_string();

    if store
        .questions
        .read()
        .contains_key(&QuestionId(relation_id.clone()))
        == false
    {
        return Err(warp::reject::custom(ParameterError::QuestionNotFound));
    }

    let answer = Answer {
        id: Uuid::new_v4().to_string(),
        content,
        question_id: relation_id,
    };

    store
        .answers
        .write()
        .insert(answer.clone().id, answer.clone());

    Ok(warp::reply::json(&answer))
    // Ok(warp::reply::with_status("Answer Added",StatusCode::OK))
}

async fn return_error(r: Rejection) -> Result<impl Reply, Rejection> {
    println!("{:?}", r);
    if let Some(error) = r.find::<CorsForbidden>() {
        Ok(warp::reply::with_status(
            error.to_string(),
            StatusCode::FORBIDDEN,
        ))
    } else if let Some(error) = r.find::<ParameterError>() {
        if *error == ParameterError::QuestionNotFound {
            return Ok(warp::reply::with_status(
                error.to_string(),
                StatusCode::NOT_FOUND,
            ));
        }
        Ok(warp::reply::with_status(
            error.to_string(),
            StatusCode::RANGE_NOT_SATISFIABLE,
        ))
    } else if let Some(error) = r.find::<BodyDeserializeError>() {
        Ok(warp::reply::with_status(
            error.to_string(),
            StatusCode::RANGE_NOT_SATISFIABLE,
        ))
    } else {
        Ok(warp::reply::with_status(
            "Route not found".to_string(),
            StatusCode::NOT_FOUND,
        ))
    }
}

#[tokio::main]
async fn main() {
    let store = Store::new();
    let store_filter = warp::any().map(move || store.clone());

    let cors = warp::cors()
        .allow_any_origin()
        .allow_header("not-in-the-request")
        .allow_methods(&[Method::PUT, Method::DELETE]);

    let get_questions = warp::get()
        .and(warp::path("questions"))
        .and(warp::path::end())
        .and(warp::query())
        .and(store_filter.clone())
        .and_then(get_questions);
    // .and_then(update_question);

    let get_single_question = warp::get()
        .and(warp::path("questions"))
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(store_filter.clone())
        .and_then(get_single_question);

    let add_question = warp::post()
        .and(warp::path("questions"))
        .and(warp::path::end())
        .and(store_filter.clone())
        .and(warp::body::json())
        .and_then(add_question);

    let update_question = warp::put()
        .and(warp::path("questions"))
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(store_filter.clone())
        .and(warp::body::json())
        .and_then(update_question);

    let delete_question = warp::delete()
        .and(warp::path("questions"))
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(store_filter.clone())
        .and_then(delete_question);

    let add_answer = warp::post()
        .and(warp::path("answers"))
        .and(warp::path::end())
        .and(store_filter.clone())
        .and(warp::body::form())
        .and_then(add_answer);

    let routes = get_questions
        .or(get_single_question)
        .or(add_question)
        .or(add_answer)
        .or(update_question)
        .or(delete_question)
        .with(cors)
        .recover(return_error);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
