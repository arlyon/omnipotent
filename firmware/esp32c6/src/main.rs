#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer, with_timeout};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock, interrupt::software::SoftwareInterruptControl, ram, rng::Rng,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_radio::wifi::{Config, ControllerConfig, Interface, WifiController, sta::StationConfig};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

// Read from the local build environment. Never log these.
const SSID: &str = env!("OMNIPOTENT_WIFI_SSID");
const PASSWORD: &str = env!("OMNIPOTENT_WIFI_PASSWORD");

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timer = TimerGroup::new(peripherals.TIMG0);
    let interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timer.timer0, interrupts.software_interrupt0);

    let station = Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into()),
    );
    let (controller, interfaces) = esp_radio::wifi::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(station),
    )
    .expect("Wi-Fi initialization failed");

    let rng = Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        interfaces.station,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(connection(controller).expect("Wi-Fi task allocation failed"));
    spawner.spawn(network(runner).expect("Network task allocation failed"));

    println!("Omnipotent C6: joining Wi-Fi; credentials are not logged");
    let mut rx = [0; 4096];
    let mut tx = [0; 4096];
    loop {
        stack.wait_config_up().await;
        if let Some(config) = stack.config_v4() {
            println!("OPDS ready: http://{}/opds", config.address.address());
        }
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        socket.set_timeout(Some(Duration::from_secs(30)));
        if socket.accept(80).await.is_err() {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        }
        let result = with_timeout(
            Duration::from_secs(30),
            omnipotent_serve::serve_connection(&mut socket, &omnipotent_serve::sample::FILES),
        )
        .await;
        if matches!(result, Ok(Ok(()))) {
            socket.close();
            let _ = with_timeout(Duration::from_secs(2), socket.flush()).await;
        }
        socket.abort();
    }
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    loop {
        if controller.connect_async().await.is_ok() {
            println!("Wi-Fi connected");
            let _ = controller.wait_for_disconnect_async().await;
            println!("Wi-Fi disconnected; reconnecting");
        } else {
            println!("Wi-Fi connection failed; retrying");
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[embassy_executor::task]
async fn network(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await;
}
