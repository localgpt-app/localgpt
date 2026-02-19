# OAuth Setup Guide

This guide explains how to configure LocalGPT to use OAuth credentials from Claude Pro/Max and Google Gemini subscription plans instead of pay-per-request API keys.

## Claude Pro/Max OAuth

### Benefits
- Use your Claude Pro ($20/month) or Max ($100/month) subscription quota
- Avoid per-token API charges
- Fixed monthly cost with generous usage limits

### Obtaining OAuth Tokens

Claude OAuth uses PKCE (Proof Key for Code Exchange) flow. You'll need to:

1. **Generate PKCE pair:**
   - Create a code_verifier (random string)
   - Generate code_challenge using S256 (SHA-256 hash of verifier)

2. **Authorization URL:**
   ```
   https://claude.ai/oauth/authorize?client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=https://console.anthropic.com/oauth/code/callback&code_challenge=<YOUR_CHALLENGE>&code_challenge_method=S256&scope=org:create_api_key user:profile user:inference&state=<RANDOM_STATE>
   ```

3. **Token Exchange:**
   After user authorizes, exchange the code for tokens:
   ```bash
   curl -X POST https://console.anthropic.com/v1/oauth/token \
     -H "Content-Type: application/json" \
     -d '{
       "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
       "grant_type": "authorization_code",
       "redirect_uri": "https://console.anthropic.com/oauth/code/callback",
       "code": "<AUTHORIZATION_CODE>",
       "code_verifier": "<YOUR_VERIFIER>"
     }'
   ```

4. **Response:**
   ```json
   {
     "access_token": "...",
     "refresh_token": "...",
     "expires_in": 3600
   }
   ```

### Configuration

Add to `~/.localgpt/config.toml`:

```toml
[providers.anthropic_oauth]
access_token = "${ANTHROPIC_OAUTH_TOKEN}"
refresh_token = "${ANTHROPIC_OAUTH_REFRESH_TOKEN}"  # Optional, for future token refresh
base_url = "https://api.anthropic.com"
```

Then set environment variables:
```bash
export ANTHROPIC_OAUTH_TOKEN="your-access-token"
export ANTHROPIC_OAUTH_REFRESH_TOKEN="your-refresh-token"
```

### Usage

Use any Claude model as normal. The OAuth provider is automatically preferred when configured:

```bash
localgpt ask "Hello" --model anthropic/claude-opus-4-5
# or
localgpt ask "Hello" --model opus  # alias
```

### References
- [anthropic-auth library (Rust)](https://github.com/querymt/anthropic-auth) - Complete OAuth implementation
- [DeepWiki: Claude OAuth Guide](https://deepwiki.com/sst/opencode-anthropic-auth/4.1-claude-promax-oauth)

---

## Google Gemini OAuth

### Benefits
- Use your Gemini subscription quota
- Enterprise/project-scoped access
- Fixed monthly cost for subscriptions

### Obtaining OAuth Tokens

1. **Enable Gemini API:**
   - Go to [Google Cloud Console](https://console.cloud.google.com/)
   - Enable "Google Generative Language API"

2. **Configure OAuth Consent Screen:**
   - Navigate to "APIs & Services > Consent Screen"
   - Set user type (External for public, Internal for organization)
   - Fill in app details and add test users

3. **Create OAuth Credentials:**
   - Go to "APIs & Services > Credentials"
   - Click "Create Credentials" → "OAuth client ID"
   - Select application type (Desktop app for CLI, Web app for web apps)
   - Download the `client_id.json` file

4. **Authorization Flow:**
   ```bash
   # Manual OAuth flow (pseudocode)
   # 1. Redirect user to Google OAuth consent screen
   # 2. User authorizes
   # 3. Exchange authorization code for tokens
   
   # Or use gemini-cli:
   gemini auth login --oauth
   ```

5. **Token Response:**
   ```json
   {
     "access_token": "...",
     "refresh_token": "...",
     "expires_in": 3600
   }
   ```

### Configuration

Add to `~/.localgpt/config.toml`:

```toml
[providers.gemini_oauth]
access_token = "${GEMINI_OAUTH_TOKEN}"
refresh_token = "${GEMINI_OAUTH_REFRESH_TOKEN}"  # Optional, for future token refresh
base_url = "https://generativelanguage.googleapis.com"
project_id = "${GOOGLE_CLOUD_PROJECT}"  # Optional, for enterprise plans
```

Set environment variables:
```bash
export GEMINI_OAUTH_TOKEN="your-access-token"
export GEMINI_OAUTH_REFRESH_TOKEN="your-refresh-token"
export GOOGLE_CLOUD_PROJECT="your-project-id"  # If using project-scoped access
```

### Usage

Use Gemini models with the `gemini/` prefix:

```bash
localgpt ask "Hello" --model gemini/gemini-2.0-flash
# or
localgpt ask "Hello" --model gemini-2.0-flash  # auto-routed
```

### References
- [Gemini OAuth Quickstart](https://ai.google.dev/gemini-api/docs/oauth)
- [Gemini CLI Authentication](https://www.geminicli.cc/docs/authentication)
- [Google Cloud Authentication](https://docs.cloud.google.com/gemini/enterprise/docs/authentication)

---

## Token Refresh (Future Enhancement)

**Note:** Automatic token refresh is not yet implemented. You must manually refresh expired tokens.

To refresh tokens:

**Claude:**
```bash
curl -X POST https://console.anthropic.com/v1/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    "grant_type": "refresh_token",
    "refresh_token": "<YOUR_REFRESH_TOKEN>"
  }'
```

**Gemini:**
```bash
curl -X POST https://oauth2.googleapis.com/token \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "client_id=<YOUR_CLIENT_ID>&client_secret=<YOUR_CLIENT_SECRET>&refresh_token=<YOUR_REFRESH_TOKEN>&grant_type=refresh_token"
```

---

## Security Notes

1. **Store tokens securely** - Use environment variables, not hardcoded values
2. **Never commit tokens** to version control
3. **Rotate tokens regularly** - Especially if compromised
4. **Use refresh tokens** - Access tokens expire, keep refresh tokens secure
5. **Limit scopes** - Only request necessary OAuth scopes

---

## Troubleshooting

### "401 Unauthorized" Error
- Your access token has expired. Use the refresh token to obtain a new one.
- Check that the token is correctly set in environment variables.

### "OAuth provider not configured" Error
- Ensure `[providers.anthropic_oauth]` or `[providers.gemini_oauth]` is in your config.
- Verify environment variables are exported in your shell.

### Token Refresh Issues
- Refresh tokens can expire after long periods of inactivity.
- You may need to re-authorize through the OAuth flow.

---

## Alternative: API Keys

If OAuth setup is too complex, you can still use traditional API keys:

```toml
# Claude API (pay-per-request)
[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

# Gemini API (pay-per-request, not available yet in LocalGPT)
# Currently only OAuth is supported for Gemini
```
