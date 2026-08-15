use p2p_provider::{LiveProviderRuntime, SOURCE_LABEL};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let runtime = match LiveProviderRuntime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("diagnostic=failed stage=initialize category={error}");
            std::process::exit(1);
        }
    };
    let result = runtime
        .check_pair("USDT", "EGP", CancellationToken::new(), |_| {})
        .await;
    match result {
        Ok(result) => {
            println!("diagnostic=passed");
            println!("source={SOURCE_LABEL}");
            println!("pair=USDT/EGP");
            println!("buy_valid={}", result.acquisition.buy.quality.valid());
            println!("sell_valid={}", result.acquisition.sell.quality.valid());
            println!(
                "buy_total={}",
                result
                    .acquisition
                    .buy
                    .quality
                    .provider_total()
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            );
            println!(
                "sell_total={}",
                result
                    .acquisition
                    .sell
                    .quality
                    .provider_total()
                    .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            );
            println!(
                "agent_trade_methods={}",
                result
                    .agent_trade_methods
                    .as_ref()
                    .map_or(0, |methods| methods.methods.len())
            );
            println!("agent_warning={}", result.agent_warning.unwrap_or("none"));
        }
        Err(error) => {
            eprintln!("diagnostic=failed category={error}");
            std::process::exit(1);
        }
    }
}
