1. cargo run --package hftbacktest --example priceaction_backtest
2. cargo run --package hftbacktest --example priceaction_live
3. cd connector && cargo run binancefutures binancefutures ./examples/binancefutures.toml
4. cargo build --target=x86_64-unknown-linux-gnu --release  --package hftbacktest --example priceaction_live
5. https://reach.stratosphere.capital/data/usdm/
6. `# add api_key and secret to connector/examples/binancefutures.toml
# stream_url = "wss://fstream.binancefuture.com/ws"
# api_url = "https://testnet.binancefuture.com"
# order_prefix = "test"

# <update gridtrading_live>

# run connector
cd connector && cargo run binancefutures binancefutures ./examples/binancefutures.toml

# run strategy
cd hftbacktest && cargo run --example gridtrading_live
`
4. hft
5. ssh
   1. sudo chmod 600 key.pem
   2. ssh -i key.pem ec2-user@15.168.174.145
   3. ssh-add -k key.pem
   4. ssh ec2-user@15.168.174.145
   5. vim /etc/ssh/sshd_config
   6. ClientAliveInterval 0->30
   7. ClientAliveCountMax 3->86400
   8. service sshd restart
   9. scp xxx ec2-user@15.168.174.145:/home/ec2-user/trader
6. cross compile
   1. rustup target add x86_64-unknown-linux-gnu  
   2. brew install FiloSottile/musl-cross/musl-cross  
   3. brew install gcc
   4. cargo build --target=x86_64-unknown-linux-gnu --release
   5. cargo build --target=x86_64-unknown-linux-gnu --release  --package hftbacktest --example priceaction_live
7. fht