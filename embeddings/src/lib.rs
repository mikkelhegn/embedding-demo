use anyhow::{Context, Result};
use env_logger;
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use spin_sdk::{
    http::{Params, Request, Response},
    http_component, http_router,
    sqlite::{self, Connection, ValueResult},
};

// A simple Spin HTTP component.
#[http_component]
fn handle_embeddings(req: Request) -> Result<Response> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    info!(
        "Received {} request at {}",
        req.method().to_string(),
        req.uri().to_string()
    );

    let router = http_router! {
        GET "/embeddings" => get_embeddings,
        POST "/embeddings" => create_embeddings,
        DELETE "/embeddings/:id" => delete_embeddings,
        _ "/*" => |_req, _params| {
            Ok(http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .body(None)
                .unwrap())
        }
    };

    router.handle(req)
}

fn get_embeddings(_req: Request, _params: Params) -> Result<Response> {
    let query = "SELECT * FROM embeddings";
    let conn = Connection::open_default()?;
    let embedding_records: Vec<Embedding> = conn
        .execute(query, &[])?
        .rows()
        .map(|row| -> anyhow::Result<Embedding> { row.try_into() })
        .collect::<anyhow::Result<Vec<Embedding>>>()?;

    info!("Rows: {:?}", embedding_records);

    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(Some(serde_json::to_string(&embedding_records)?.into()))
        .unwrap())
}

fn create_embeddings(_req: Request, _params: Params) -> Result<Response> {
    let my_embedding = Embedding {
        id: 0,
        reference: String::from("My ref"),
        text: String::from("My text"),
        embedding: Some(vec![1.23, 4.56, 7.89]),
    };
    let vec = json!(my_embedding.embedding);
    let blob = serde_json::to_vec(&vec)?;

    let query = "INSERT INTO embeddings (reference, text, embedding) VALUES(?, ?, ?) RETURNING id;";
    let query_params = [
        sqlite::ValueParam::Text(my_embedding.reference.as_str()),
        sqlite::ValueParam::Text(my_embedding.text.as_str()),
        sqlite::ValueParam::Blob(blob.as_slice()),
    ];
    let conn = Connection::open_default()?;
    let result = conn.execute(query, &query_params)?;
    info!("Result: {:?}", result.rows[0]);
    Ok(http::Response::builder()
        .status(http::StatusCode::CREATED)
        .body(None)
        .unwrap())
}

fn delete_embeddings(_req: Request, _params: Params) -> Result<Response> {
    // Delete the record with id

    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(None)
        .unwrap())
}

#[derive(Debug, Serialize, Deserialize)]
struct Embedding {
    id: u32,
    reference: String,
    text: String,
    embedding: Option<Vec<f32>>,
}

impl<'a> TryFrom<sqlite::Row<'a>> for Embedding {
    type Error = anyhow::Error;

    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        let id = row.get::<u32>("id").unwrap();
        let reference = row
            .get::<&str>("reference")
            .context("reference column is empty")?;
        let text = row.get::<&str>("text").context("text column is empty")?;
        let embedding: Vec<f32>;
        match row.get::<&ValueResult>("embedding").unwrap() {
            ValueResult::Blob(b) => {
                embedding = serde_json::from_value(serde_json::from_slice(b.as_slice()).unwrap())?;
            },
            _ => todo!(),
        };        
        Ok(Self {
            id,
            reference: reference.to_owned(),
            text: text.to_owned(),
            embedding: Some(embedding),
        })
    }
}
