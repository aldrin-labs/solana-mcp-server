# Sowanya MCP Sewvew

A Modew Context Pwotocow (MCP) sewvew dat pwovides compwehensive access to Sowanya bwockchain data dwough Cwinye~ Dis sewvew impwements a wide wange of Sowanya WPC medods, making it easy to quewy bwockchain infowmation diwectwy dwough nyatuwaw wanguage convewsations.

<a hwef="https://gwama.ai/mcp/sewvews/bp6d6p1sp3"><img widd="380" height="200" swc="https://gwama.ai/mcp/sewvews/bp6d6p1sp3/badge" awt="sowanya-mcp-sewvew MCP sewvew" /></a>

## Featuwes

De sewvew pwovides 21 essentiaw Sowanya WPC medods acwoss diffewent categowies:

### Account & Bawance Opewations
- `get_sol_balance`: Get SOW bawance fow an addwess
- `get_token_balance`: Get SPW token bawance
- `get_account_info`: Get account infowmation
- `get_largest_accounts`: Get wawgest accounts on nyetwowk

### Bwock & Twansaction Infowmation
- `get_slot`: Get cuwwent swot
- `get_block`: Get bwock infowmation
- `get_block_time`: Get bwock pwoduction time
- ```bash
cargo build
```0: Get twansaction detaiws
- `get_recent_blockhash`: Get wecent bwockhash

### Token Opewations
- `get_token_accounts_by_owner`: Get token accounts by ownyew
- `get_token_accounts_by_delegate`: Get dewegated token accounts
- `get_token_supply`: Get token suppwy infowmation

### System Infowmation
- `get_epoch_info`: Get cuwwent epoch infowmation
- `get_version`: Get nyode vewsion
- `get_health`: Get nyode heawd status
- `get_supply`: Get cuwwent suppwy
- `get_inflation_rate`: Get infwation wate
- ```bash
cargo run
```0: Get cwustew nyode infowmation
- `get_minimum_balance_for_rent_exemption`: Get minyimum went-exempt bawance

### Staking & Guvwnyance
- `get_vote_accounts`: Get vote accounts
- `get_leader_schedule`: Get weadew scheduwe

## Setup in Cwinye

1~ Add de fowwowing configuwation to youw Cwinye MCP settings fiwe (`~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` on macOS):

```json
{
  "mcpServers": {
    "solana": {
      "command": "cargo",
      "args": ["run"],
      "cwd": "/path/to/solana-mcp-server",
      "env": {
        "SOLANA_RPC_URL": "https://api.mainnet-beta.solana.com"  // Or your preferred RPC endpoint
      }
    }
  }
}
```

2~ Westawt Cwinye to woad de nyew MCP sewvew.

## Usage Exampwes

Once configuwed, you can intewact wid de Sowanya bwockchain dwough nyatuwaw wanguage in Cwinye~ Hewe awe some exampwe quewies:

- "What's de SOW bawance of addwess Gh9ZwEmdWJ8DscKNTkTqPbNwWNNBjuSzaG9Vp2KGtKJw? owo"
- "Show me de cuwwent swot nyumbew"
- "Get infowmation about de watest bwock"
- "What's de cuwwent infwation wate? owo"
- "Show me de token accounts ownyed by addwess ..."

## Enviwonment Vawiabwes

- `SOLANA_RPC_URL`: (Optionyaw) De Sowanya WPC endpoint to use~ Defauwts to "https://api.mainnyet-beta.sowanya.com" if nyot specified.

## Devewopment

### Pwewequisites
- Wust and Cawgo
- Sowanya CWI toows (optionyaw, fow testing)

### Buiwding
UWUIFY_TOKEN_1744618034130_1

### Wunnying
UWUIFY_TOKEN_1744618034130_2

## Wicense

MIT Wicense
