# Twilio Setup (SMS & WhatsApp)

Twilio integration supports both SMS and WhatsApp messaging.

## 1. Create a Twilio Account

Sign up at [twilio.com](https://www.twilio.com/) and note your **Account SID** and **Auth Token** from the dashboard.

## 2. Get a Phone Number

- **SMS**: Purchase a phone number in the Twilio console
- **WhatsApp**: Set up a WhatsApp sender in the Twilio console (or use the sandbox for testing)

## 3. Store Credentials

Add credentials to `~/.borg/config.toml`:

```toml
[credentials]
# Option A: environment variables
TWILIO_ACCOUNT_SID = "TWILIO_ACCOUNT_SID"
TWILIO_AUTH_TOKEN = "TWILIO_AUTH_TOKEN"
TWILIO_PHONE_NUMBER = "TWILIO_PHONE_NUMBER"           # for SMS
TWILIO_WHATSAPP_NUMBER = "TWILIO_WHATSAPP_NUMBER"     # for WhatsApp

# Option B: macOS Keychain
TWILIO_AUTH_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "twilio-auth", "-w"] }
```

## 4. Enable the Gateway

```toml
[gateway]
host = "127.0.0.1"
port = 7842
```

## 5. Set the Webhook URL

Expose a public URL (e.g. via ngrok):

```sh
ngrok http 7842
```

In the Twilio console:

- **SMS**: Under your phone number's configuration, set the **Messaging webhook** to:
  ```
  https://your-domain.ngrok-free.app/webhook/sms
  ```
- **WhatsApp**: Under the WhatsApp sandbox or sender configuration, set the webhook to:
  ```
  https://your-domain.ngrok-free.app/webhook/whatsapp
  ```

The gateway also accepts `/webhook/twilio` as a generic endpoint for both.

## 6. Start the Gateway

```sh
borg gateway
```

The gateway also runs automatically as part of the daemon.

## 7. Verify

Send an SMS to your Twilio number or a WhatsApp message to your WhatsApp-enabled number. You should get a response from your agent.

## Features

- SMS and WhatsApp messaging via Twilio API
- HMAC-SHA1 request signature verification
- Circuit breaker for reliability
- Automatic message chunking

## Additional configuration

### Access control

```toml
[gateway.channel_policies]
twilio = "pairing"   # pairing (default) | open | disabled
```
