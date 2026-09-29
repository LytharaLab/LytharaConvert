// 本地服务保留控制台窗口：它既是运行日志，也是「服务还活着」的唯一指示。
#[tokio::main]
async fn main() {
    lythara_convert_lib::serve().await
}
