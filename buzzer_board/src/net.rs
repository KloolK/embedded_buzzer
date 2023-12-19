use core::sync::atomic::Ordering;

use crate::{singleton, LedOutputs, NetPeripherals, BUTTON_PRESS_Q, INIT_TIME};
use common::{ButtonPress, Message};
use defmt::*;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::{tcp::Error::ConnectionReset, Ipv4Address, Ipv4Cidr, Stack, StackResources};
use embassy_stm32::eth::PacketQueue;
use embassy_stm32::eth::{generic_smi::GenericSMI, Ethernet};
use embassy_stm32::gpio::Level;
use embassy_stm32::interrupt;
use embassy_stm32::peripherals::ETH;
use embassy_time::{Duration, Instant, Timer};
use embedded_io::asynch::{Read, Write};
use embedded_nal_async::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpConnect};
use heapless::Vec;
use postcard::{from_bytes, to_vec};

pub type Device = Ethernet<'static, ETH, GenericSMI>;

const THROTTLE_TIME: Duration = Duration::from_millis(10);

#[embassy_executor::task]
pub async fn net_task(stack: &'static Stack<Device>) -> ! {
    stack.run().await
}

pub fn init_net_stack(net_p: NetPeripherals, seed: u64) -> &'static Stack<Device> {
    let eth_int = interrupt::take!(ETH);
    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let device = Ethernet::new(
        singleton!(PacketQueue::<16, 16>::new()),
        net_p.eth,
        eth_int,
        net_p.pa1,
        net_p.pc3,
        net_p.pa2,
        net_p.pc1,
        net_p.pa7,
        net_p.pc4,
        net_p.pc5,
        net_p.pb0,
        net_p.pb1,
        net_p.pg13,
        net_p.pg12,
        net_p.pc2,
        net_p.pe2,
        net_p.pg11,
        GenericSMI,
        mac_addr,
        1,
    );

    // Set laptop IP to 192.168.100.1 and listen with `netcat -l 8000`
    let config = embassy_net::Config::Static(embassy_net::StaticConfig {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 100, 5), 24),
        dns_servers: Vec::new(),
        gateway: Some(Ipv4Address::new(192, 168, 100, 1)),
    });

    // Init network stack
    let stack = &*singleton!(Stack::new(
        device,
        config,
        singleton!(StackResources::<2>::new()),
        seed
    ));

    stack
}

#[embassy_executor::task]
pub async fn rx_task(stack: &'static Stack<Device>, mut led_outputs: LedOutputs) -> ! {
    static STATE: TcpClientState<1, 1024, 1024> = TcpClientState::new();
    let client = TcpClient::new(&stack, &STATE);

    'outer: loop {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 8001));

        info!("Trying to connect receiver task...");
        let r = client.connect(addr).await;
        if let Err(e) = r {
            error!("Connection error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        let mut connection = r.unwrap();
        info!("Receiver task connected!");

        let mut buf = [0u8; 1000];
        let mut cursor_pos = 0;

        let mut remaining_err_on_zero = 3;

        loop {
            match connection.read(&mut buf[cursor_pos..]).await {
                Ok(0) => {
                    // Nothing new read, try to deserialize
                    match from_bytes::<Message>(&buf[0..cursor_pos]) {
                        Ok(message) => {
                            handle_message(message, &mut led_outputs);
                        }
                        Err(_) => {
                            warn!("Could not deserialize, skipping");
                            if remaining_err_on_zero <= 0 {
                                warn!("Reconnecting");
                                continue 'outer;
                            }
                            remaining_err_on_zero -= 1;
                        }
                    }
                    cursor_pos = 0;

                    Timer::after(Duration::from_secs(1)).await;
                }
                Ok(num_read) => {
                    cursor_pos += num_read;

                    match from_bytes::<Message>(&buf[0..cursor_pos]) {
                        Ok(message) => {
                            handle_message(message, &mut led_outputs);
                            cursor_pos = 0;
                        }
                        Err(_) => {
                            warn!("Could not deserialize, skipping");
                        }
                    }
                }
                Err(e) => {
                    warn!("Error while reading: {}", e);
                    if e == ConnectionReset {
                        break;
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn tx_task(stack: &'static Stack<Device>) -> ! {
    static STATE: TcpClientState<1, 1024, 1024> = TcpClientState::new();
    let client = TcpClient::new(&stack, &STATE);

    loop {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 100, 1), 8000));

        info!("Trying to connect sender task...");
        let r = client.connect(addr).await;
        if let Err(e) = r {
            error!("Connection error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        let mut connection = r.unwrap();
        info!("Sender task connected!");

        loop {
            let reset_time = INIT_TIME.load(Ordering::Acquire);
            match BUTTON_PRESS_Q.dequeue() {
                None => Timer::after(THROTTLE_TIME).await,
                Some((button_id, press_time)) => {
                    let millis_since_init = (press_time as u32).saturating_sub(reset_time);
                    if millis_since_init == 0 {
                        warn!(
                            "Button press {} registered before last reset, skipping",
                            button_id
                        );
                        continue;
                    }

                    let message = Message::ButtonPress(ButtonPress {
                        button_id,
                        millis_since_init,
                    });

                    debug!("Sending message: {:?}", message);

                    let serialized: Vec<u8, 30> = to_vec(&message).unwrap();

                    let r = connection.write_all(&serialized).await;
                    if let Err(e) = r {
                        info!("write error: {:?}", e);

                        if e == ConnectionReset {
                            break;
                        }

                        continue;
                    }
                }
            }
        }
    }
}

fn handle_message(message: Message, led_outputs: &mut LedOutputs) {
    match message {
        Message::InitBoard | Message::InitReactionGame(_) => {
            info!("Received InitBoard instruction");
            let instant_millis = Instant::now().as_millis() as u32;
            INIT_TIME.store(instant_millis, Ordering::Release);
        }
        Message::Ping(ping_nr) => {
            info!("Received Ping({})", ping_nr);
        }
        Message::ButtonPress(_) => {
            warn!("Board should not be receiving ButtonPress data");
        }
        Message::LedUpdate(update) => {
            info!("Received LED update: {:?}", update);
            if let Some(led) = led_outputs.get_mut(update.button_id as usize) {
                if update.on {
                    led.set_level(Level::High);
                } else {
                    led.set_level(Level::Low);
                }
            }
        }
    }
}
