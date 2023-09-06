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
    // Make the level configurable
    env_logger::builder()
        .filter_level(Info)
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

fn get_embeddings(req: Request, _params: Params) -> Result<Response> {
    match req.uri().query().is_some() {
        true => {
            let query = serde_qs::from_str(req.uri().query().unwrap_or_default()).unwrap();

            let result_set = get_similar(query);

            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Some(serde_json::to_vec(&result_set.unwrap())?.into()))
                .unwrap())
        }
        false => {
            info!("In else");
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
                .body(Some(serde_json::to_string(&embedding_records)?.into()))
                .unwrap())
        }
    }
}

fn create_embeddings(req: Request, _params: Params) -> Result<Response> {
    let embedding_request: EmbeddingRequest = serde_json::from_slice(
        req.body()
            .as_ref()
            .map(|b| -> &[u8] { b })
            .unwrap_or_default(),
    )
    .unwrap();

    let embeddings = generate_and_store_embedding(embedding_request);

    trace!("Result: {:?}", embeddings);

    Ok(http::Response::builder()
        .status(http::StatusCode::CREATED)
        .body(None)
        .unwrap())
}

fn delete_embeddings(_req: Request, params: Params) -> Result<Response> {
    let conn = Connection::open_default()?;
    let query_params = [sqlite::ValueParam::Integer(
        params.get("id").unwrap().parse().unwrap(),
    )];
    let _ = conn.execute("DELETE FROM embeddings WHERE id = (?)", &query_params);

    // Report appropriate status code - e.g., if delete fails
    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(None)
        .unwrap())
}

fn generate_and_store_embedding(embedding_req: EmbeddingRequest) -> Result<Vec<Embedding>> {
    let mut embeddings = embedding_req.embeddings;
    let text: Vec<&str> = embeddings.iter().map(|t| t.text.as_str()).collect();
    let embedding_result = generate_embeddings(AllMiniLmL6V2, &text[..]).unwrap();

    trace!("Generated embeddings: {:?}", embedding_result);

    let conn = Connection::open_default()?;

    for (i, e) in embeddings.iter_mut().enumerate() {
        if let Some(embedding) = embedding_result.embeddings.get(i) {
            e.embedding = Some(embedding.clone());

            let vec = json!(e.embedding);
            let blob = serde_json::to_vec(&vec)?;

            let query_params = [
                sqlite::ValueParam::Text(e.reference.as_deref().unwrap_or_default()),
                sqlite::ValueParam::Text(e.text.as_str()),
                sqlite::ValueParam::Blob(blob.as_slice()),
            ];

            let result = conn.execute(
                "INSERT INTO embeddings ('reference', 'text', 'embedding') VALUES (?, ?, ?);",
                &query_params,
            );

            trace!("Result: {:?}", result);
        }
    }
    // Some error handling needed...
    Ok(embeddings)
}

impl<'a> TryFrom<sqlite::Row<'a>> for Embedding {
    type Error = anyhow::Error;

    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        // TODO: Fix unwraps()
        let id = Some(row.get::<u32>("id").unwrap());
        let reference = row
            .get::<&str>("reference")
            .context("reference column is empty")?;
        let text = row.get::<&str>("text").context("text column is empty")?;
        let embedding: Vec<f32> = match row.get::<&ValueResult>("embedding").unwrap() {
            ValueResult::Blob(b) => serde_json::from_value(serde_json::from_slice(b.as_slice()).unwrap())?,
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

fn get_similar(query: Query) -> Result<SimilarityResultSet> {
    let text: Vec<&str> = vec![query.text.as_str()];
    let embedding = generate_embeddings(AllMiniLmL6V2, &text[..]);
    let sql_query = "SELECT * FROM embeddings";
    let conn = Connection::open_default()?;
    let embedding_records: Vec<Embedding> = conn
        .execute(sql_query, &[])?
        .rows()
        .map(|row| -> anyhow::Result<Embedding> { row.try_into() })
        .collect::<anyhow::Result<Vec<Embedding>>>()?;

    let mut result_set = SimilarityResultSet {
        text: query.text.to_string(),
        results: Vec::new(),
    };

    for e in embedding_records.iter() {
        let similarity = cosine_similarity(
            &e.embedding.clone().unwrap(),
            embedding.clone().unwrap().embeddings.get(0).unwrap(),
        );
        let mut result = SimilarityResult {
            similarity,
            embedding: e.clone(),
        };
        result.embedding.embedding = None;
        result_set.results.push(result);
    }

    result_set
        .results
        .sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

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

#[derive(Deserialize)]
struct EmbeddingRequest {
    embeddings: Vec<Embedding>,
    //options: Option<<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct Query {
    text: String,
    //options: Option<HashMap<String, String>>,
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
