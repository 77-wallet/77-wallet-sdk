use rumqttc::v5::{AsyncClient, MqttOptions};
use std::time::Duration;
use wallet_database::factory::RepositoryFactory;
use wallet_utils::init_test_log;

use wallet_api::{
    domain,
    service::device::{DeviceService, APP_ID},
    test::env::{setup_test_environment, TestData},
};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let TestData {
        wallet_manager: _, ..
    } = setup_test_environment(None, None, false, None)
        .await
        .unwrap();

    init_test_log();

    let pool = wallet_api::Context::get_global_sqlite_pool().unwrap();
    let repo = RepositoryFactory::repo(pool.clone());
    let device_service = DeviceService::new(repo);
    let device = device_service.get_device_info().await.unwrap().unwrap();

    let content = domain::app::DeviceDomain::device_content(&device).unwrap();
    let client_id = domain::app::DeviceDomain::client_id_by_device(&device).unwrap();
    tracing::info!("client: {client_id:?}");
    // let client_id = "704e9cbabd98f596d26e42f9ceb4091a".to_string();

    let property = vec![
        ("content".to_owned(), content),
        ("clientId".to_owned(), client_id.clone()),
    ];

    let mut connect_props = rumqttc::v5::mqttbytes::v5::ConnectProperties::new();
    connect_props.session_expiry_interval = Some(2 * 60 * 60);

    let mqtt_url = "mqtt://100.105.62.76:1883";
    let url = format!("{}?client_id={}", mqtt_url, client_id);
    let mut mqttoptions = MqttOptions::parse_url(url).unwrap();
    mqttoptions
        .set_credentials(device.sn, APP_ID)
        .set_connect_properties(connect_props)
        .set_keep_alive(Duration::from_secs(20))
        .set_transport(rumqttc::Transport::Tcp)
        .set_user_properties(property)
        .set_connection_timeout(20)
        .set_clean_start(false)
        .set_manual_acks(false);

    let (_, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    loop {
        let event = eventloop.poll().await.unwrap();
        match event {
            rumqttc::v5::Event::Incoming(packet) => match packet {
                rumqttc::v5::mqttbytes::v5::Packet::Connect(_connect, _last_will, _login) => {
                    println!("xxxxxxxxxxxxxxxxxx");
                }
                rumqttc::v5::mqttbytes::v5::Packet::ConnAck(conn_ack) => {
                    tracing::info!("建立链接 = {conn_ack:?}");
                }
                rumqttc::v5::mqttbytes::v5::Packet::Publish(publish) => {
                    tracing::info!("收到消息 = {publish:?}");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PubAck(_pub_ack) => {
                    println!("pub_ack");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PingReq(_ping_req) => {
                    println!("ping req");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PingResp(_ping_resp) => {
                    tracing::info!("ping ");
                }
                rumqttc::v5::mqttbytes::v5::Packet::Subscribe(_subscribe) => {
                    println!("subscribe");
                }
                rumqttc::v5::mqttbytes::v5::Packet::SubAck(_sub_ack) => {
                    println!("suback");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PubRec(_pub_rec) => {
                    println!("pubrec");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PubRel(_pub_rel) => {
                    println!("pubrel");
                }
                rumqttc::v5::mqttbytes::v5::Packet::PubComp(_pub_comp) => {
                    println!("pubcomp");
                }
                rumqttc::v5::mqttbytes::v5::Packet::Unsubscribe(_unsubscribe) => {
                    println!("unsubscribe");
                }
                rumqttc::v5::mqttbytes::v5::Packet::UnsubAck(_unsub_ack) => {
                    println!("unsuback");
                }
                rumqttc::v5::mqttbytes::v5::Packet::Disconnect(_disconnect) => {
                    println!("disconnect");
                }
            },
            rumqttc::v5::Event::Outgoing(outgoing) => {
                tracing::info!("Outgoing: {:?}", outgoing);
            }
        }
    }
}
