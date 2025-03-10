use anyhow::Result;
use async_trait::async_trait;
use log::{error, info, warn};
use mcp_sdk::{
    transport::{Transport, JsonRpcMessage, StdioTransport}, 
    types::{
        CallToolRequest, CallToolResponse, Tool, ToolResponseContent, ErrorCode,
        Resource, ResourceContents, ListResourcesRequest, ListResourceTemplatesRequest,
        ReadResourceRequest, ResourceContent, InitializationOptions, ServerCapabilities,
        ResourceCapabilities,
    },
    server::Server,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
};
use std::{str::FromStr, time::{Duration, Instant}};
use tokio::time::timeout;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::sleep;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REQUESTS_PER_MINUTE: u32 = 60;
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(1);
const CACHE_DURATION: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct CachedResource {
    content: Vec<ResourceContent>,
    timestamp: Instant,
}

struct CircuitBreaker {
    failures: std::sync::atomic::AtomicU32,
    last_failure: std::sync::Mutex<Instant>,
    threshold: u32,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    fn new(threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            failures: std::sync::atomic::AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(Instant::now()),
            threshold,
            reset_timeout,
        }
    }

    fn record_failure(&self) -> bool {
        let mut last_failure = self.last_failure.lock().unwrap();
        let current_time = Instant::now();
        
        if current_time.duration_since(*last_failure) > self.reset_timeout {
            self.failures.store(0, std::sync::atomic::Ordering::SeqCst);
        }
        
        *last_failure = current_time;
        let failures = self.failures.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        failures >= self.threshold
    }

    fn is_open(&self) -> bool {
        let failures = self.failures.load(std::sync::atomic::Ordering::SeqCst);
        let last_failure = *self.last_failure.lock().unwrap();
        
        failures >= self.threshold && 
        Instant::now().duration_since(last_failure) <= self.reset_timeout
    }

    fn reset(&self) {
        self.failures.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

struct SolanaMcpServer {
    rpc_client: RpcClient,
    request_count: std::sync::atomic::AtomicU32,
    last_reset: std::sync::Mutex<Instant>,
    resources: Vec<Resource>,
    resource_templates: Vec<ResourceTemplate>,
    resource_cache: Arc<RwLock<HashMap<String, CachedResource>>>,
    circuit_breaker: CircuitBreaker,
    transport: Arc<RwLock<Option<Box<dyn Transport>>>>,
}

#[async_trait]
impl Server for SolanaMcpServer {
    async fn connect(&self, transport: Box<dyn Transport>, options: InitializationOptions) -> Result<()> {
        info!("Connecting with capabilities: {:?}", options.capabilities);
        transport.open()?;
        *self.transport.write().await = Some(transport);
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<Tool>> {
        Ok(vec![
            Tool {
                name: "get_slot".to_string(),
                description: Some("Get the current slot".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_block_time".to_string(),
                description: Some("Get the estimated production time of a block".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "slot": {
                            "type": "integer",
                            "description": "Slot number"
                        }
                    },
                    "required": ["slot"]
                }),
            },
            Tool {
                name: "get_transaction".to_string(),
                description: Some("Get transaction details".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "signature": {
                            "type": "string",
                            "description": "Transaction signature"
                        }
                    },
                    "required": ["signature"]
                }),
            },
            Tool {
                name: "get_block".to_string(),
                description: Some("Get block information".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "slot": {
                            "type": "integer",
                            "description": "Slot number"
                        }
                    },
                    "required": ["slot"]
                }),
            },
            Tool {
                name: "get_account_info".to_string(),
                description: Some("Get account information".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "address": {
                            "type": "string",
                            "description": "Account public key"
                        }
                    },
                    "required": ["address"]
                }),
            },
            Tool {
                name: "get_program_accounts".to_string(),
                description: Some("Get all accounts owned by a program".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "program_id": {
                            "type": "string",
                            "description": "Program ID (public key)"
                        }
                    },
                    "required": ["program_id"]
                }),
            },
            Tool {
                name: "get_recent_blockhash".to_string(),
                description: Some("Get a recent block hash".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_version".to_string(),
                description: Some("Get Solana version information".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_health".to_string(),
                description: Some("Get node health status".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_minimum_balance_for_rent_exemption".to_string(),
                description: Some("Get minimum balance required for rent exemption".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data_size": {
                            "type": "integer",
                            "description": "Size of account data"
                        }
                    },
                    "required": ["data_size"]
                }),
            },
            Tool {
                name: "get_supply".to_string(),
                description: Some("Get information about the current supply".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_largest_accounts".to_string(),
                description: Some("Get the largest accounts".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_inflation_rate".to_string(),
                description: Some("Get the current inflation rate".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_cluster_nodes".to_string(),
                description: Some("Get information about all the nodes participating in the cluster".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_token_accounts_by_owner".to_string(),
                description: Some("Get all token accounts owned by the specified account".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "owner": {
                            "type": "string",
                            "description": "Owner's public key"
                        }
                    },
                    "required": ["owner"]
                }),
            },
            Tool {
                name: "get_token_accounts_by_delegate".to_string(),
                description: Some("Get all token accounts that delegate authority to the specified account".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "delegate": {
                            "type": "string",
                            "description": "Delegate's public key"
                        }
                    },
                    "required": ["delegate"]
                }),
            },
            Tool {
                name: "get_token_supply".to_string(),
                description: Some("Get token supply information".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "mint": {
                            "type": "string",
                            "description": "Token mint address"
                        }
                    },
                    "required": ["mint"]
                }),
            },
            Tool {
                name: "get_vote_accounts".to_string(),
                description: Some("Get information about all vote accounts".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "get_leader_schedule".to_string(),
                description: Some("Get the leader schedule for the current epoch".to_string()),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        ])
    }
}

impl SolanaMcpServer {
    fn new() -> Self {
        let circuit_breaker = CircuitBreaker::new(5, Duration::from_secs(60));
        let resource_cache = Arc::new(RwLock::new(HashMap::new()));
        let transport = Arc::new(RwLock::new(None));
        let rpc_url = std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| String::from("https://api.mainnet-beta.solana.com"));
        
        info!("Initializing Solana MCP server with RPC URL: {}", rpc_url);
        
        let resources = vec![
            Resource {
                uri: "solana://supply".to_string(),
                name: "Current Supply".to_string(),
                description: Some("Current circulating supply information".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            Resource {
                uri: "solana://inflation".to_string(),
                name: "Inflation Rate".to_string(),
                description: Some("Current inflation rate information".to_string()),
                mime_type: Some("application/json".to_string()),
            },
        ];

        let resource_templates = vec![
            ResourceTemplate {
                uri_template: "solana://accounts/{address}".to_string(),
                name: "Account Information".to_string(),
                description: Some("Get information about a Solana account".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            ResourceTemplate {
                uri_template: "solana://transactions/{signature}".to_string(),
                name: "Transaction Information".to_string(),
                description: Some("Get information about a transaction".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            ResourceTemplate {
                uri_template: "solana://blocks/{slot}".to_string(),
                name: "Block Information".to_string(),
                description: Some("Get information about a block".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            ResourceTemplate {
                uri_template: "solana://tokens/{mint}/supply".to_string(),
                name: "Token Supply".to_string(),
                description: Some("Get supply information for a token".to_string()),
                mime_type: Some("application/json".to_string()),
            },
        ];
        
        Self {
            rpc_client: RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed()),
            request_count: std::sync::atomic::AtomicU32::new(0),
            last_reset: std::sync::Mutex::new(Instant::now()),
            resources,
            resource_templates,
            resource_cache,
            circuit_breaker,
            transport,
        }
    }

    async fn retry_with_backoff<F, T>(&self, future: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>> + Clone,
    {
        let mut retries = 0;
        loop {
            let future_clone = future.clone();
            match future_clone.await {
                Ok(result) => {
                    self.circuit_breaker.reset();
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        if self.circuit_breaker.record_failure() {
                            error!("Circuit breaker opened due to too many failures");
                        }
                        return Err(e);
                    }
                    warn!("Operation failed, retrying in {:?}: {}", RETRY_DELAY, e);
                    sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    async fn update_cache(&self, uri: &str, content: Vec<ResourceContent>) {
        let mut cache = self.resource_cache.write().await;
        cache.insert(uri.to_string(), CachedResource {
            content,
            timestamp: Instant::now(),
        });
    }

    fn check_rate_limit(&self) -> Result<()> {
        let mut last_reset = self.last_reset.lock().unwrap();
        let current_time = Instant::now();
        let elapsed = current_time.duration_since(*last_reset);

        if elapsed >= Duration::from_secs(60) {
            self.request_count.store(0, std::sync::atomic::Ordering::SeqCst);
            *last_reset = current_time;
        }

        let count = self.request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count >= MAX_REQUESTS_PER_MINUTE {
            warn!("Rate limit exceeded: {} requests in the last minute", count);
            return Err(anyhow::anyhow!("Rate limit exceeded"));
        }

        Ok(())
    }

    async fn handle_read_resource(&self, request: ReadResourceRequest) -> Result<Vec<ResourceContent>> {
        if self.circuit_breaker.is_open() {
            return Err(anyhow::anyhow!("Service temporarily unavailable"));
        }

        let uri = request.uri.as_str();
        
        // Check cache first
        if let Some(cached) = self.resource_cache.read().await.get(uri) {
            if cached.timestamp.elapsed() < CACHE_DURATION {
                return Ok(cached.content.clone());
            }
        }
        
        // If not in cache or expired, fetch fresh data
        let result = match uri {
            "solana://supply" => {
                let supply = self.retry_with_backoff(async {
                    let supply = self.rpc_client.get_supply()?;
                    Ok(serde_json::to_string(&supply)?)
                }).await?;
                
                let content = vec![ResourceContent {
                    uri: uri.to_string(),
                    text: Some(supply),
                    blob: None,
                    mime_type: Some("application/json".to_string()),
                }];
                self.update_cache(uri, content.clone()).await;
                Ok(content)
            }
            "solana://inflation" => {
                let rate = self.retry_with_backoff(async {
                    let rate = self.rpc_client.get_inflation_rate()?;
                    Ok(serde_json::to_string(&rate)?)
                }).await?;
                
                let content = vec![ResourceContent {
                    uri: uri.to_string(),
                    text: Some(rate),
                    blob: None,
                    mime_type: Some("application/json".to_string()),
                }];
                self.update_cache(uri, content.clone()).await;
                Ok(content)
            }
            _ => {
                // Handle dynamic resources
                if uri.starts_with("solana://accounts/") {
                    let address = uri.strip_prefix("solana://accounts/").unwrap();
                    let pubkey = Pubkey::from_str(address)
                        .map_err(|_| anyhow::anyhow!("Invalid account address"))?;
                    
                    let info = self.retry_with_backoff(async {
                        let account = self.rpc_client.get_account(&pubkey)?;
                        Ok(serde_json::to_string(&account)?)
                    }).await?;
                    
                    let content = vec![ResourceContent {
                        uri: uri.to_string(),
                        text: Some(info),
                        blob: None,
                        mime_type: Some("application/json".to_string()),
                    }];
                    self.update_cache(uri, content.clone()).await;
                    Ok(content)
                } else if uri.starts_with("solana://transactions/") {
                    let signature = uri.strip_prefix("solana://transactions/").unwrap();
                    
                    let tx_info = self.retry_with_backoff(async {
                        let sig = signature.parse()
                            .map_err(|_| anyhow::anyhow!("Invalid transaction signature"))?;
                        let tx = self.rpc_client.get_transaction(&sig, None)?;
                        Ok(serde_json::to_string(&tx)?)
                    }).await?;
                    
                    let content = vec![ResourceContent {
                        uri: uri.to_string(),
                        text: Some(tx_info),
                        blob: None,
                        mime_type: Some("application/json".to_string()),
                    }];
                    self.update_cache(uri, content.clone()).await;
                    Ok(content)
                } else if uri.starts_with("solana://blocks/") {
                    let slot = uri.strip_prefix("solana://blocks/").unwrap()
                        .parse::<u64>()
                        .map_err(|_| anyhow::anyhow!("Invalid slot number"))?;
                    
                    let block_info = self.retry_with_backoff(async {
                        let block = self.rpc_client.get_block(slot)?;
                        Ok(serde_json::to_string(&block)?)
                    }).await?;
                    
                    let content = vec![ResourceContent {
                        uri: uri.to_string(),
                        text: Some(block_info),
                        blob: None,
                        mime_type: Some("application/json".to_string()),
                    }];
                    self.update_cache(uri, content.clone()).await;
                    Ok(content)
                } else if uri.starts_with("solana://tokens/") {
                    let parts: Vec<&str> = uri.strip_prefix("solana://tokens/").unwrap().split('/').collect();
                    if parts.len() == 2 && parts[1] == "supply" {
                        let mint = Pubkey::from_str(parts[0])
                            .map_err(|_| anyhow::anyhow!("Invalid mint address"))?;
                        
                        let supply = self.retry_with_backoff(async {
                            let supply = self.rpc_client.get_token_supply(&mint)?;
                            Ok(serde_json::to_string(&supply)?)
                        }).await?;
                        
                        let content = vec![ResourceContent {
                            uri: uri.to_string(),
                            text: Some(supply),
                            blob: None,
                            mime_type: Some("application/json".to_string()),
                        }];
                        self.update_cache(uri, content.clone()).await;
                        Ok(content)
                    } else {
                        Err(anyhow::anyhow!("Invalid token resource URI"))
                    }
                } else {
                    Err(anyhow::anyhow!("Resource not found"))
                }
            }
        };

        if result.is_err() && self.circuit_breaker.record_failure() {
            error!("Circuit breaker opened due to too many failures");
        }

        result
    }

    async fn handle_request(&self, request: CallToolRequest) -> Result<CallToolResponse> {
        if self.circuit_breaker.is_open() {
            return Err(anyhow::anyhow!("Service temporarily unavailable"));
        }

        // Check rate limit first
        self.check_rate_limit()?;

        let start_time = Instant::now();
        let result = timeout(REQUEST_TIMEOUT, self.handle_tool_request(request.clone())).await;
        let duration = start_time.elapsed();

        match result {
            Ok(response) => {
                info!("Tool {} executed successfully in {:?}", request.name, duration);
                response
            }
            Err(_) => {
                error!("Tool {} timed out after {:?}", request.name, duration);
                if self.circuit_breaker.record_failure() {
                    error!("Circuit breaker opened due to timeout");
                }
                Err(anyhow::anyhow!("Request timed out"))
            }
        }
    }

    async fn handle_tool_request(&self, request: CallToolRequest) -> Result<CallToolResponse> {
        match request.name.as_str() {
            "get_slot" => {
                let slot = self.retry_with_backoff(async {
                    Ok(self.rpc_client.get_slot()?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Current slot: {}", slot),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_block_time" => {
                let slot = request.arguments.as_ref()
                    .and_then(|args| args.get("slot"))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid slot parameter"))?;
                
                let timestamp = self.retry_with_backoff(async {
                    Ok(self.rpc_client.get_block_time(slot)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Block time: {} (Unix timestamp)", timestamp),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_transaction" => {
                let signature = request.arguments.as_ref()
                    .and_then(|args| args.get("signature"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing signature parameter"))?;
                
                let tx_info = self.retry_with_backoff(async {
                    let sig = signature.parse()
                        .map_err(|_| anyhow::anyhow!("Invalid transaction signature"))?;
                    let tx = self.rpc_client.get_transaction(&sig, None)?;
                    Ok(serde_json::to_string(&tx)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Transaction info: {}", tx_info),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_block" => {
                let slot = request.arguments.as_ref()
                    .and_then(|args| args.get("slot"))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid slot parameter"))?;
                
                let block_info = self.retry_with_backoff(async {
                    let block = self.rpc_client.get_block(slot)?;
                    Ok(serde_json::to_string(&block)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Block info: {}", block_info),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_account_info" => {
                let address = request.arguments.as_ref()
                    .and_then(|args| args.get("address"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing address parameter"))?;
                
                let pubkey = Pubkey::from_str(address)
                    .map_err(|_| anyhow::anyhow!("Invalid account address"))?;
                
                let info = self.retry_with_backoff(async {
                    let account = self.rpc_client.get_account(&pubkey)?;
                    Ok(serde_json::to_string(&account)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Account info: {}", info),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_program_accounts" => {
                let program_id = request.arguments.as_ref()
                    .and_then(|args| args.get("program_id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing program_id parameter"))?;
                
                let pubkey = Pubkey::from_str(program_id)
                    .map_err(|_| anyhow::anyhow!("Invalid program ID"))?;
                
                let accounts = self.retry_with_backoff(async {
                    let accounts = self.rpc_client.get_program_accounts(&pubkey)?;
                    Ok(serde_json::to_string(&accounts)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Program accounts: {}", accounts),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_recent_blockhash" => {
                let info = self.retry_with_backoff(async {
                    let (blockhash, fee_calculator) = self.rpc_client.get_recent_blockhash()?;
                    Ok(serde_json::to_string(&serde_json::json!({
                        "blockhash": blockhash.to_string(),
                        "fee_calculator": fee_calculator
                    }))?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Recent blockhash info: {}", info),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_version" => {
                let version = self.retry_with_backoff(async {
                    let version = self.rpc_client.get_version()?;
                    Ok(serde_json::to_string(&version)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Version info: {}", version),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_health" => {
                let health = self.retry_with_backoff(async {
                    match self.rpc_client.get_health() {
                        Ok(_) => Ok("ok".to_string()),
                        Err(e) => Ok(format!("error: {}", e))
                    }
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Node health: {}", health),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_minimum_balance_for_rent_exemption" => {
                let data_size = request.arguments.as_ref()
                    .and_then(|args| args.get("data_size"))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid data_size parameter"))? as usize;
                
                let balance = self.retry_with_backoff(async {
                    Ok(self.rpc_client.get_minimum_balance_for_rent_exemption(data_size)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Minimum balance for rent exemption: {} lamports", balance),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_supply" => {
                let supply = self.retry_with_backoff(async {
                    let supply = self.rpc_client.get_supply()?;
                    Ok(serde_json::to_string(&supply)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Supply info: {}", supply),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_largest_accounts" => {
                let accounts = self.retry_with_backoff(async {
                    let accounts = self.rpc_client.get_largest_accounts()?;
                    Ok(serde_json::to_string(&accounts)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Largest accounts: {}", accounts),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_inflation_rate" => {
                let rate = self.retry_with_backoff(async {
                    let rate = self.rpc_client.get_inflation_rate()?;
                    Ok(serde_json::to_string(&rate)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Inflation rate: {}", rate),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_cluster_nodes" => {
                let nodes = self.retry_with_backoff(async {
                    let nodes = self.rpc_client.get_cluster_nodes()?;
                    Ok(serde_json::to_string(&nodes)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Cluster nodes: {}", nodes),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_token_accounts_by_owner" => {
                let owner = request.arguments.as_ref()
                    .and_then(|args| args.get("owner"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing owner parameter"))?;
                
                let pubkey = Pubkey::from_str(owner)
                    .map_err(|_| anyhow::anyhow!("Invalid owner address"))?;
                
                let accounts = self.retry_with_backoff(async {
                    let accounts = self.rpc_client.get_token_accounts_by_owner(
                        &pubkey,
                        solana_client::rpc_config::RpcTokenAccountsFilter::ProgramId(spl_token::id()),
                    )?;
                    Ok(serde_json::to_string(&accounts)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Token accounts: {}", accounts),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_token_accounts_by_delegate" => {
                let delegate = request.arguments.as_ref()
                    .and_then(|args| args.get("delegate"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing delegate parameter"))?;
                
                let pubkey = Pubkey::from_str(delegate)
                    .map_err(|_| anyhow::anyhow!("Invalid delegate address"))?;
                
                let accounts = self.retry_with_backoff(async {
                    let accounts = self.rpc_client.get_token_accounts_by_delegate(
                        &pubkey,
                        solana_client::rpc_config::RpcTokenAccountsFilter::ProgramId(spl_token::id()),
                    )?;
                    Ok(serde_json::to_string(&accounts)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Delegated accounts: {}", accounts),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_token_supply" => {
                let mint = request.arguments.as_ref()
                    .and_then(|args| args.get("mint"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing mint parameter"))?;
                
                let pubkey = Pubkey::from_str(mint)
                    .map_err(|_| anyhow::anyhow!("Invalid mint address"))?;
                
                let supply = self.retry_with_backoff(async {
                    let supply = self.rpc_client.get_token_supply(&pubkey)?;
                    Ok(serde_json::to_string(&supply)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Token supply: {}", supply),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_vote_accounts" => {
                let accounts = self.retry_with_backoff(async {
                    let accounts = self.rpc_client.get_vote_accounts()?;
                    Ok(serde_json::to_string(&accounts)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Vote accounts: {}", accounts),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "get_leader_schedule" => {
                let schedule = self.retry_with_backoff(async {
                    let schedule = self.rpc_client.get_leader_schedule(None)?;
                    Ok(serde_json::to_string(&schedule)?)
                }).await?;
                
                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!("Leader schedule: {}", schedule),
                    }],
                    is_error: Some(false),
                    meta: None,
                })
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", request.name))
        }
    }
}

#[async_trait]
impl Transport for SolanaMcpServer {
    fn send<'a>(&'a self, message: &'a JsonRpcMessage) -> Result<()> {
        if let Some(transport) = &*self.transport.blocking_read() {
            transport.send(message)
        } else {
            Err(anyhow::anyhow!("Transport not initialized"))
        }
    }

    fn receive<'a>(&'a self) -> Result<JsonRpcMessage> {
        if let Some(transport) = &*self.transport.blocking_read() {
            transport.receive()
        } else {
            Err(anyhow::anyhow!("Transport not initialized"))
        }
    }

    fn open(&self) -> Result<()> {
        info!("Opening transport connection");
        Ok(())
    }

    fn close(&self) -> Result<()> {
        info!("Closing transport connection");
        if let Some(transport) = &*self.transport.blocking_read() {
            transport.close()?;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();
    
    info!("Starting Solana MCP server");
    let server = SolanaMcpServer::new();
    
    // Initialize transport with proper capabilities
    let transport = StdioTransport::new();
    
    // Declare server capabilities
    let initialization_options = InitializationOptions {
        server_name: "solana-mcp-server".to_string(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: ServerCapabilities {
            tools: Some(true),
            resources: Some(ResourceCapabilities {
                list_changed: Some(true),
                templates: Some(true),
            }),
            prompts: None,
            sampling: None,
            roots: None,
            experimental: None,
        },
    };
    
    info!("Solana MCP server running on stdio");
    
    // Connect transport with declared capabilities
    if let Err(e) = server.connect(transport, initialization_options).await {
        error!("Failed to connect transport: {}", e);
        return Err(e.into());
    }
    
    loop {
        match server.receive() {
            Ok(message) => {
                if let Err(e) = server.send(&message) {
                    error!("Error sending response: {}", e);
                }
            }
            Err(e) => {
                error!("Error receiving message: {}", e);
                // Consider breaking the loop on fatal errors
                if e.to_string().contains("EOF") {
                    break;
                }
            }
        }
    }

    info!("Shutting down Solana MCP server");
    Ok(())
}
