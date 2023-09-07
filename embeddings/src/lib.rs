use anyhow::{Context, Result};
use log::{info, trace, LevelFilter::Info};
use serde::{Deserialize, Serialize};
use serde_json::*;
use spin_sdk::{
    http::{Params, Request, Response},
    http_component, http_router,
    llm::{generate_embeddings, EmbeddingModel::AllMiniLmL6V2},
    sqlite::{self, Connection, ValueResult},
};

#[http_component]
fn handle_embeddings(req: Request) -> Result<Response> {
    env_logger::builder().filter_level(Info).init();

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

fn get_embeddings(req: Request, _params: Params) -> Result<Response> {
    match req.uri().query() {
        Some(query) => {
            let query: Query = serde_qs::from_str(query)?;
            let text = vec![query.text.as_str()];
            let result_set = get_similar(text)?;

            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Some(serde_json::to_vec(&result_set)?.into()))?)
        }
        None => {
            let query = "SELECT * FROM embeddings";
            let conn = Connection::open_default()?;
            let embedding_records: Vec<Embedding> = conn
                .execute(query, &[])?
                .rows()
                .map(|row| -> anyhow::Result<Embedding> { row.try_into() })
                .collect::<anyhow::Result<Vec<Embedding>>>()?;

            trace!("Rows: {:?}", embedding_records);
            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .body(Some(serde_json::to_string(&embedding_records)?.into()))?)
        }
    }
}

fn create_embeddings(req: Request, _params: Params) -> Result<Response> {
    let embedding_request: Vec<Embedding> = serde_json::from_slice(
        req.body()
            .as_deref()
            .map(|b| -> &[u8] { b })
            .unwrap_or_default(),
    )
    .unwrap();

    match generate_and_store_embedding(embedding_request) {
        Ok(num_rec) => {
            info!("Stored {:?} records", num_rec);
        }
        Err(e) => {
            trace!("Failed to store record: {:?}", e);
        }
    };

    Ok(http::Response::builder()
        .status(http::StatusCode::CREATED)
        .body(None)
        .unwrap())
}

fn delete_embeddings(_req: Request, params: Params) -> Result<Response> {
    let status = match params.get("id") {
        Some(id) => {
            let query_params = [sqlite::ValueParam::Integer(id.parse()?)];
            let conn = Connection::open_default()?;
            let _ = conn.execute("DELETE FROM embeddings WHERE id = (?)", &query_params);
            http::StatusCode::OK
        }
        None => http::StatusCode::NOT_FOUND,
    };

    Ok(http::Response::builder().status(status).body(None)?)
}

fn generate_and_store_embedding(embedding_request: Vec<Embedding>) -> Result<usize> {
    let text: Vec<&str> = embedding_request.iter().map(|e| e.text.as_str()).collect();
    let embedding_result = generate_embeddings(AllMiniLmL6V2, &text)?;

    trace!("Generated embeddings: {:?}", embedding_result);

    let conn = Connection::open_default()?;

    for (e, res) in embedding_request.iter().zip(embedding_result.embeddings) {
        let vec = json!(res.clone());
        let blob = serde_json::to_vec(&vec)?;

        let query_params = [
            sqlite::ValueParam::Text(e.reference.as_deref().unwrap_or_default()),
            sqlite::ValueParam::Text(e.text.as_str()),
            sqlite::ValueParam::Blob(blob.as_slice()),
        ];

        let _ = conn.execute(
            "INSERT INTO embeddings ('reference', 'text', 'embedding') VALUES (?, ?, ?);",
            &query_params,
        );
    }

    Ok(embedding_request.len())
}

impl<'a> TryFrom<sqlite::Row<'a>> for Embedding {
    type Error = anyhow::Error;

    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        let id = Some(row.get::<u32>("id").unwrap_or_default());
        let reference = row
            .get::<&str>("reference")
            .context("reference column is empty")?;
        let text = row.get::<&str>("text").context("text column is empty")?;
        let embedding: Vec<f32> = match row.get::<&ValueResult>("embedding").unwrap() {
            ValueResult::Blob(b) => {
                serde_json::from_value(serde_json::from_slice(b.as_slice()).unwrap_or_default())?
            }
            _ => todo!(),
        };
        Ok(Self {
            id,
            reference: Some(reference.to_owned()),
            text: text.to_owned(),
            embedding: Some(embedding),
        })
    }
}

fn get_similar(query: Vec<&str>) -> Result<SimilarityResultSet> {
    let sql_query = "SELECT * FROM embeddings";
    let conn = Connection::open_default()?;
    let embedding_records: Vec<Embedding> = conn
        .execute(sql_query, &[])?
        .rows()
        .map(|row| -> anyhow::Result<Embedding> { row.try_into() })
        .collect::<anyhow::Result<Vec<Embedding>>>()?;
    info!("String to compare: {:?}", query[0].to_string());
    let mut result_set = SimilarityResultSet {
        text: query[0].to_string(),
        results: Vec::new(),
    };
    let embedding_result = generate_embeddings(AllMiniLmL6V2, &query);

    if let Some(embedding_to_compare) = embedding_result?.embeddings.get(0) {
        for e in embedding_records.into_iter() {
            let similarity = cosine_similarity(e.embedding.as_ref().unwrap(), embedding_to_compare);
            let mut result = SimilarityResult {
                similarity,
                embedding: e,
            };
            result.embedding.embedding = None;
            result_set.results.push(result);
        }

        result_set
            .results
            .sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    }
    Ok(result_set)
}

fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f32 {
    let dot_product = vec1
        .iter()
        .zip(vec2.iter())
        .map(|(x, y)| x * y)
        .sum::<f32>();
    let norm1 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm2 = vec2.iter().map(|y| y * y).sum::<f32>().sqrt();
    dot_product / (norm1 * norm2)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Embedding {
    id: Option<u32>,
    reference: Option<String>,
    text: String,
    embedding: Option<Vec<f32>>,
}

#[derive(Serialize)]
struct SimilarityResultSet {
    text: String,
    results: Vec<SimilarityResult>,
}

#[derive(Serialize)]
struct SimilarityResult {
    embedding: Embedding,
    similarity: f32,
}

#[derive(Deserialize)]
struct Query {
    text: String,
}
