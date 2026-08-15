# Experimental provider contract

## Status and boundary

P2P Desk’s sole advertisement dataset is persistently identified as **Experimental Binance P2P Web**. It is a Binance website search contract, not a published stable developer API. It can change, rate-limit, block, or become unavailable without notice.

A failure is terminal for the current live acquisition. Cached observations, local history, Agent results, another source, or fabricated data are never substituted, merged, or labeled live.

The trusted Rust crate `crates/p2p-provider` owns all remote access. The webview has no network API or remote `connect-src` permission.

## Fixed destinations

The adapter can call only these compile-time HTTPS destinations:

| Role                   | Destination                                                          | Dataset effect                            |
| ---------------------- | -------------------------------------------------------------------- | ----------------------------------------- |
| Primary ads            | `https://p2p.binance.com/bapi/c2c/v2/friendly/c2c/adv/search`        | Sole advertisement dataset                |
| Agent payment metadata | `https://www.binance.com/bapi/c2c/v1/public/c2c/agent/trade-methods` | Separate optional metadata/health only    |
| Agent P2P quote        | `https://www.binance.com/bapi/c2c/v1/public/c2c/agent/quote-price`   | Separate optional health cross-check only |

No caller supplies a URL. Reqwest is configured HTTPS-only, does not follow redirects or emit a referer, uses 10-second connect and 30-second total deadlines, and reads at most 4 MiB per response.

## Side invariant

| User intent | Primary request `tradeType` | Required returned advertiser side |
| ----------- | --------------------------- | --------------------------------- |
| Buy asset   | `BUY`                       | `adv.tradeType=SELL`              |
| Sell asset  | `SELL`                      | `adv.tradeType=BUY`               |

Every row must also match the requested asset and fiat. A wrong side or cross-pair row fails the complete operation and opens a persistent contract circuit; it is never reinterpreted.

## Request and exact-value policy

Requests use page, 20 rows, corrected side, canonical asset/fiat, optional canonical exact fiat transaction amount, and a payment filter only when exactly one method is selected. Multiple methods are omitted upstream because their website semantics are not treated as a stable contract; later domain eligibility applies the user’s visible ANY/ALL choice locally.

Source money and percentages enter as JSON string or number lexemes and parse directly to the exact decimal domain type. No provider numeric field passes through `f32` or `f64`. Completion and positive-rate fractions are bounded to 0–1 and converted exactly to percentages. Effective maximum fiat is the conservative minimum of fixed and dynamic maxima when both exist.

Unknown response fields are ignored rather than persisted. Required malformed records are counted by non-sensitive rejection category. Wrong side, cross-pair, malformed envelope, and invalid total are hard failures. Provider display text has controls removed and is length bounded; the frontend renders it only through ordinary text nodes, and unsafe HTML sinks are prohibited by audit.

## Pagination, queue, and completion

- Page size: 20.
- Result target: exact validated/deduplicated 20–1000 per side.
- Maximum: 50 pages per side.
- Request order while both sides need data: Buy p1, Sell p1, Buy p2, Sell p2, and so on.
- One complete acquisition graph runs at a time.
- One provider request is in flight globally, including Agent metadata.
- Starts are at least 500 ms apart: nominal maximum 2 requests/second, burst 1.
- Duplicate IDs are counted and suppressed.
- A full page with no new ID is a repeated/no-progress contract error.
- A short/empty page succeeds only when consistent with the provider total.
- Completion requires target reached or trustworthy total exhaustion on both sides.
- Asymmetric one-side zero and all-rows-rejected are explicit errors; confirmed two-side zero is a distinct valid empty result.

Progress contains stages, active user intent, page, attempts, request count, fetched/valid/duplicate/rejected counters, target, total, and exhaustion. It contains no raw body, nickname, full provider identifier, or transaction/account data.

## Retry and circuit policy

Primary pages attempt at most three times. Only connection/request timeout, transient network failure, HTTP 408, and selected 5xx statuses retry. Backoff is cancellation-aware, deterministically jittered around one then two seconds, and uses integer duration arithmetic.

Ordinary 4xx, rejected request, malformed contract, wrong pair, and wrong side do not retry.

- HTTP 429 opens the global circuit for `Retry-After` or at least 60 seconds.
- HTTP 403/418 WAF or ban response opens it for `Retry-After` or at least 15 minutes.
- Envelope/schema break opens a persistent schema circuit.
- Wrong-side/cross-pair break opens a persistent side circuit.
- A persistent circuit can close only around an explicit serialized diagnostic and remains closed only after that complete two-side diagnostic succeeds.

Cancellation can interrupt queue waiting, rate pacing, the HTTP future, retry delay, or graph progression. No partial acquisition object is returned.

## Pair checks and Agent isolation

Add/check pair accepts canonical distinct symbols and runs a corrected two-side primary acquisition at the minimum target. Only a complete primary result creates a `VerifiedPair` model with adapter version, verification time, and observed payment identifiers. Disabled entries require an explicit reason and time. SQLite persistence is intentionally added by the persistence gate.

Agent trade methods and quote have separate Rust types with no conversion into a primary normalized ad or acquisition. Agent failure becomes a separately labeled warning after primary success and never repairs, substitutes, or masks primary failure. Both Agent calls share the global request gate and circuit.

## Fixtures and diagnostics

Contract tests use only synthetic values created for tests. No captured raw response, real nickname, or real ad/merchant ID is stored in fixtures, source, logs, or evidence. The live diagnostic prints only source/pair, aggregate valid counts, provider totals, Agent method count, and warning category.

`cargo run --manifest-path crates/p2p-provider/Cargo.toml --locked --example provider_diagnostic` runs the current USDT/EGP schema diagnostic. It is evidence of current compatibility only, not a stability promise or final live-ready acceptance.
