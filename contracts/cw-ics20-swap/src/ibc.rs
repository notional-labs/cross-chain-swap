use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, BankMsg, Binary, CosmosMsg, DepsMut, Env,
    IbcBasicResponse, IbcChannel, IbcChannelCloseMsg, IbcChannelConnectMsg, IbcChannelOpenMsg,
    IbcEndpoint, IbcOrder, IbcPacket, IbcPacketAckMsg, IbcPacketReceiveMsg, IbcPacketTimeoutMsg,
    IbcReceiveResponse, Reply, Response, SubMsg, SubMsgResult,
};

use crate::amount::Amount;
use crate::error::{ContractError, Never};
use crate::ibc_msg::{Ics20Ack, Ics20Packet, OsmoPacket, SwapPacket, Voucher};
use crate::parse::{parse_gamm_result, GammResult, SWAP_ATTR, SWAP_EVENT};
use crate::state::{
    increase_channel_balance, reduce_channel_balance, restore_balance_reply, ChannelInfo,
    ReplyArgs, CHANNEL_INFO, REPLY_ARGS,
};
use cw_utils::parse_execute_response_data;
use osmo_proto::osmosis::gamm::v1beta1::MsgSwapExactAmountInResponse as SwapResponse;
use osmo_proto::proto_ext::MessageExt;

pub const ICS20_VERSION: &str = "ics20-1";
pub const ICS20_ORDERING: IbcOrder = IbcOrder::Unordered;

// create a serialized success message
fn ack_success_with_body(data: Binary) -> Binary {
    let res = Ics20Ack::Result(data);
    to_binary(&res).unwrap()
}

// create a serialized success message
fn ack_success() -> Binary {
    let res = Ics20Ack::Result(b"1".into());
    to_binary(&res).unwrap()
}

// create a serialized error message
fn ack_fail(err: String) -> Binary {
    let res = Ics20Ack::Error(err);
    to_binary(&res).unwrap()
}

const RECEIVE_ID: u64 = 1337;
const SWAP_ID: u64 = 0xcb37;
const ACK_FAILURE_ID: u64 = 0xfa17;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> Result<Response, ContractError> {
    match reply.id {
        RECEIVE_ID => reply_receive(deps, reply),
        SWAP_ID => reply_gamm_result::<SwapResponse>(deps, reply, SWAP_EVENT, SWAP_ATTR),
        ACK_FAILURE_ID => reply_ack_on_error(reply),
        _ => Err(ContractError::UnknownReplyId { id: reply.id }),
    }
}

pub fn reply_gamm_result<M: GammResult + osmo_proto::Message + std::default::Default>(
    deps: DepsMut,
    reply: Reply,
    event: &str,
    attribute: &str,
) -> Result<Response, ContractError> {
    match reply.result {
        SubMsgResult::Ok(tx) => {
            let gamm_res = parse_gamm_result::<M>(tx, event, attribute);
            match gamm_res {
                Ok(ack) => {
                    let reply_args = REPLY_ARGS.load(deps.storage)?;
                    // increase gamm amount out
                    increase_channel_balance(
                        deps.storage,
                        &reply_args.channel,
                        &ack.denom,
                        ack.amount,
                    )?;
                    let data = to_binary(&ack).unwrap();
                    Ok(Response::new().set_data(ack_success_with_body(data)))
                }
                Err(err) => {
                    restore_balance_reply(deps.storage)?;
                    Ok(Response::new().set_data(ack_fail(err.to_string())))
                }
            }
        }
        SubMsgResult::Err(err) => {
            restore_balance_reply(deps.storage)?;
            Ok(Response::new().set_data(ack_fail(err)))
        }
    }
}

pub fn reply_ack_from_data(deps: DepsMut, reply: Reply) -> Result<Response, ContractError> {
    match reply.result {
        SubMsgResult::Ok(tx) => {
            let data = tx.data.ok_or(ContractError::MissingReplyData {})?;
            let data = parse_execute_response_data(data.as_slice())?
                .data
                .ok_or(ContractError::MissingReplyData {})?;

            Ok(Response::new().set_data(ack_success_with_body(data)))
        }
        SubMsgResult::Err(err) => {
            restore_balance_reply(deps.storage)?;
            Ok(Response::new().set_data(ack_fail(err)))
        }
    }
}

pub fn reply_ack_on_error(reply: Reply) -> Result<Response, ContractError> {
    match reply.result {
        SubMsgResult::Ok(_) => Ok(Response::new()),
        SubMsgResult::Err(err) => Ok(Response::new().set_data(ack_fail(err))),
    }
}

pub fn reply_receive(deps: DepsMut, reply: Reply) -> Result<Response, ContractError> {
    match reply.result {
        SubMsgResult::Ok(_) => Ok(Response::new()),
        SubMsgResult::Err(err) => {
            restore_balance_reply(deps.storage)?;
            Ok(Response::new().set_data(ack_fail(err)))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
/// enforces ordering and versioning constraints
pub fn ibc_channel_open(
    _deps: DepsMut,
    _env: Env,
    msg: IbcChannelOpenMsg,
) -> Result<(), ContractError> {
    enforce_order_and_version(msg.channel(), msg.counterparty_version())?;

    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
/// record the channel in CHANNEL_INFO
pub fn ibc_channel_connect(
    deps: DepsMut,
    _env: Env,
    msg: IbcChannelConnectMsg,
) -> Result<IbcBasicResponse, ContractError> {
    // we need to check the counter party version in try and ack (sometimes here)
    enforce_order_and_version(msg.channel(), msg.counterparty_version())?;

    let channel: IbcChannel = msg.into();
    let info = ChannelInfo {
        id: channel.endpoint.channel_id,
        counterparty_endpoint: channel.counterparty_endpoint,
        connection_id: channel.connection_id,
    };
    CHANNEL_INFO.save(deps.storage, &info.id, &info)?;

    Ok(IbcBasicResponse::default())
}

fn enforce_order_and_version(
    channel: &IbcChannel,
    counterparty_version: Option<&str>,
) -> Result<(), ContractError> {
    if channel.version.as_str() != ICS20_VERSION {
        return Err(ContractError::InvalidIbcVersion {
            version: channel.version.clone(),
        });
    }
    if let Some(version) = counterparty_version {
        if version != ICS20_VERSION {
            return Err(ContractError::InvalidIbcVersion {
                version: version.to_string(),
            });
        }
    }
    if channel.order != ICS20_ORDERING {
        return Err(ContractError::OnlyOrderedChannel {});
    }
    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_channel_close(
    _deps: DepsMut,
    _env: Env,
    channel: IbcChannelCloseMsg,
) -> Result<IbcBasicResponse, ContractError> {
    match channel {
        IbcChannelCloseMsg::CloseConfirm { .. } => Ok(IbcBasicResponse::new()),
        IbcChannelCloseMsg::CloseInit { .. } => Err(ContractError::CannotClose {}),
        _ => panic!(),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn ibc_packet_receive(
    deps: DepsMut,
    env: Env,
    msg: IbcPacketReceiveMsg,
) -> Result<IbcReceiveResponse, Never> {
    let packet = msg.packet;

    do_ibc_packet_receive(deps, env, &packet).or_else(|err| {
        Ok(IbcReceiveResponse::new()
            .set_ack(ack_fail(err.to_string()))
            .add_attributes(vec![
                attr("action", "receive"),
                attr("success", "false"),
                attr("error", err.to_string()),
            ]))
    })
}

// Returns local denom if the denom is an encoded voucher from the expected endpoint
// Otherwise, error
fn parse_voucher(
    voucher_denom: String,
    remote_endpoint: &IbcEndpoint,
) -> Result<Voucher, ContractError> {
    let split_denom: Vec<&str> = voucher_denom.splitn(3, '/').collect();
    if split_denom.len() != 3 {
        return Err(ContractError::NoForeignTokens {});
    }
    // a few more sanity checks
    if split_denom[0] != remote_endpoint.port_id {
        return Err(ContractError::FromOtherPort {
            port: split_denom[0].into(),
        });
    }
    if split_denom[1] != remote_endpoint.channel_id {
        return Err(ContractError::FromOtherChannel {
            channel: split_denom[1].into(),
        });
    }

    Ok(Voucher {
        denom: split_denom[2].to_string(),
    })
}

// this does the work of ibc_packet_receive, we wrap it to turn errors into acknowledgements
fn do_ibc_packet_receive(
    deps: DepsMut,
    env: Env,
    packet: &IbcPacket,
) -> Result<IbcReceiveResponse, ContractError> {
    let msg: Ics20Packet = from_binary(&packet.data)?;
    let channel = packet.dest.channel_id.clone();

    // If the token originated on the remote chain, it looks like "ucosm".
    // If it originated on our chain, it looks like "port/channel/ucosm".
    let voucher = parse_voucher(msg.denom, &packet.src)?;
    let denom = voucher.denom.as_str();

    reduce_channel_balance(deps.storage, &channel, denom, msg.amount)?;

    // we need to save the data to update the balances in reply
    let reply_args = ReplyArgs {
        channel: channel.clone(),
        denom: denom.to_string(),
        amount: msg.amount,
        sender: msg.sender.clone(),
    };
    REPLY_ARGS.save(deps.storage, &reply_args)?;
    let to_send = Amount::from_parts(denom.to_string(), msg.amount);

    if let Some(action) = msg.action {
        let contract = env.contract.address.into();
        match action {
            OsmoPacket::Swap(swap) => swap_receive(swap, msg.sender, to_send, contract),
        }
    } else {
        let send = send_amount(to_send, msg.receiver.clone());
        let submsg = SubMsg::reply_on_error(send, RECEIVE_ID);

        let res = IbcReceiveResponse::new()
            .set_ack(ack_success())
            .add_submessage(submsg)
            .add_attribute("action", "receive")
            .add_attribute("sender", msg.sender)
            .add_attribute("receiver", msg.receiver)
            .add_attribute("denom", denom)
            .add_attribute("amount", msg.amount)
            .add_attribute("success", "true");

        Ok(res)
    }
}

fn swap_receive(
    swap: SwapPacket,
    sender: String,
    token_in: Amount,
    contract: String,
) -> Result<IbcReceiveResponse, ContractError> {
    let tx = osmo_proto::osmosis::gamm::v1beta1::MsgSwapExactAmountIn {
        sender: contract,
        token_in: Some(osmo_proto::cosmos::base::v1beta1::Coin {
            denom: token_in.denom(),
            amount: token_in.amount().to_string(),
        }),
        routes: swap
            .routes
            .iter()
            .map(|r| osmo_proto::osmosis::gamm::v1beta1::SwapAmountInRoute {
                token_out_denom: r.token_out_denom.to_owned(),
                pool_id: r.pool_id.u64(),
            })
            .collect(),
        token_out_min_amount: swap.token_out_min_amount.to_string(),
    };

    let submsg = SubMsg::reply_always(tx.to_msg()?, SWAP_ID);

    let res = IbcReceiveResponse::new()
        .set_ack(ack_success())
        .add_submessage(submsg)
        .add_attribute("action", "receive_swap")
        .add_attribute("sender", sender)
        .add_attribute("denom", token_in.denom())
        .add_attribute("amount", token_in.amount())
        .add_attribute("success", "true");

    Ok(res)
}

fn nonpayable(amount: &Amount) -> Result<(), ContractError> {
    if amount.is_empty() {
        Ok(())
    } else {
        Err(ContractError::NonPayable {})
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
/// check if success or failure and update balance, or return funds
pub fn ibc_packet_ack(
    deps: DepsMut,
    _env: Env,
    msg: IbcPacketAckMsg,
) -> Result<IbcBasicResponse, ContractError> {
    let ics20msg: Ics20Ack = from_binary(&msg.acknowledgement.data)?;
    match ics20msg {
        Ics20Ack::Result(_) => on_packet_success(msg.original_packet),
        Ics20Ack::Error(err) => on_packet_failure(deps, msg.original_packet, err),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
/// return fund to original sender (same as failure in ibc_packet_ack)
pub fn ibc_packet_timeout(
    deps: DepsMut,
    _env: Env,
    msg: IbcPacketTimeoutMsg,
) -> Result<IbcBasicResponse, ContractError> {
    let packet = msg.packet;
    on_packet_failure(deps, packet, "timeout".to_string())
}

// update the balance stored on this (channel, denom) index
fn on_packet_success(packet: IbcPacket) -> Result<IbcBasicResponse, ContractError> {
    let msg: Ics20Packet = from_binary(&packet.data)?;

    // similar event messages like ibctransfer module
    let attributes = vec![
        attr("action", "acknowledge"),
        attr("sender", &msg.sender),
        attr("receiver", &msg.receiver),
        attr("denom", &msg.denom),
        attr("amount", msg.amount),
        attr("success", "true"),
    ];

    Ok(IbcBasicResponse::new().add_attributes(attributes))
}

// return the tokens to sender
fn on_packet_failure(
    deps: DepsMut,
    packet: IbcPacket,
    err: String,
) -> Result<IbcBasicResponse, ContractError> {
    let msg: Ics20Packet = from_binary(&packet.data)?;

    reduce_channel_balance(deps.storage, &packet.src.channel_id, &msg.denom, msg.amount)?;

    let to_send = Amount::from_parts(msg.denom.clone(), msg.amount);
    let send = send_amount(to_send, msg.sender.clone());
    let submsg = SubMsg::reply_on_error(send, ACK_FAILURE_ID);

    // similar event messages like ibctransfer module
    let res = IbcBasicResponse::new()
        .add_submessage(submsg)
        .add_attribute("action", "acknowledge")
        .add_attribute("sender", msg.sender)
        .add_attribute("receiver", msg.receiver)
        .add_attribute("denom", msg.denom)
        .add_attribute("amount", msg.amount.to_string())
        .add_attribute("success", "false")
        .add_attribute("error", err);

    Ok(res)
}

fn send_amount(amount: Amount, recipient: String) -> CosmosMsg {
    match amount {
        Amount::Native(coin) => BankMsg::Send {
            to_address: recipient,
            amount: vec![coin],
        }
        .into(),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_helpers::*;

    use crate::contract::{execute, query_channel};
    use crate::ibc_msg::{AmountResultAck, SwapAmountInRoute};
    use crate::msg::{ExecuteMsg, TransferMsg};
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{
        coins, to_vec, Coin, Event, IbcEndpoint, ReplyOn, StdError, StdResult, SubMsgResponse,
        Timestamp, Uint128, Uint64, WasmMsg,
    };
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    #[test]
    fn check_ack_json() {
        let success = Ics20Ack::Result(b"1".into());
        let fail = Ics20Ack::Error("bad coin".into());

        let success_json = String::from_utf8(to_vec(&success).unwrap()).unwrap();
        assert_eq!(r#"{"result":"MQ=="}"#, success_json.as_str());

        let fail_json = String::from_utf8(to_vec(&fail).unwrap()).unwrap();
        assert_eq!(r#"{"error":"bad coin"}"#, fail_json.as_str());
    }

    #[test]
    fn check_packet_json() {
        let packet = Ics20Packet::new(
            Uint128::new(12345),
            "ucosm",
            "cosmos1zedxv25ah8fksmg2lzrndrpkvsjqgk4zt5ff7n",
            "wasm1fucynrfkrt684pm8jrt8la5h2csvs5cnldcgqc",
        );
        // Example message generated from the SDK
        let expected = r#"{"amount":"12345","denom":"ucosm","receiver":"wasm1fucynrfkrt684pm8jrt8la5h2csvs5cnldcgqc","sender":"cosmos1zedxv25ah8fksmg2lzrndrpkvsjqgk4zt5ff7n"}"#;

        let encoded = String::from_utf8(to_vec(&packet).unwrap()).unwrap();
        assert_eq!(expected, encoded.as_str());
    }

    fn native_payment(amount: u128, denom: &str, recipient: &str) -> SubMsg {
        SubMsg::reply_on_error(
            BankMsg::Send {
                to_address: recipient.into(),
                amount: coins(amount, denom),
            },
            RECEIVE_ID,
        )
    }

    fn check_gamm_submsg(msg: SubMsg, reply_id: u64, action: &str) -> StdResult<()> {
        if msg.id != reply_id {
            return Err(StdError::generic_err("Invalid reply id"));
        }

        if msg.reply_on != ReplyOn::Always {
            return Err(StdError::generic_err("Invalid reply on"));
        }

        match msg.msg {
            CosmosMsg::Stargate { type_url, .. } => {
                if !type_url.to_lowercase().contains(action) {
                    return Err(StdError::generic_err(format!(
                        "Invalid stargate proto url: {}",
                        type_url
                    )));
                }
            }
            _ => return Err(StdError::generic_err("Invalid cosmMsg")),
        };

        Ok(())
    }

    fn get_ack_result<T: DeserializeOwned>(data: &Binary) -> StdResult<T> {
        let ack: Ics20Ack = from_binary(data).unwrap();
        match ack {
            Ics20Ack::Result(data) => Ok(from_binary(&data).unwrap()),
            Ics20Ack::Error(err) => Err(StdError::generic_err(err)),
        }
    }

    fn mock_reply_msg(id: u64, events: Vec<Event>, data: Option<Binary>) -> Reply {
        Reply {
            id,
            result: SubMsgResult::Ok(SubMsgResponse { events, data }),
        }
    }

    fn mock_ics20_data(
        amount: u128,
        denom: &str,
        receiver: &str,
        action: Option<OsmoPacket>,
    ) -> Ics20Packet {
        Ics20Packet {
            // this is returning a foreign (our) token, thus denom is <port>/<channel>/<denom>
            denom: format!("{}/{}/{}", REMOTE_PORT, "channel-1234", denom),
            amount: amount.into(),
            sender: "remote-sender".to_string(),
            receiver: receiver.to_string(),
            action,
        }
    }

    fn mock_receive_packet(
        my_channel: &str,
        amount: u128,
        denom: &str,
        receiver: &str,
    ) -> IbcPacketReceiveMsg {
        let data = mock_ics20_data(amount, denom, receiver, None);

        mock_ibc_rcv_packet(my_channel, &data)
    }

    fn mock_ibc_rcv_packet(my_channel: &str, data: &impl Serialize) -> IbcPacketReceiveMsg {
        IbcPacketReceiveMsg::new(IbcPacket::new(
            to_binary(&data).unwrap(),
            IbcEndpoint {
                port_id: REMOTE_PORT.to_string(),
                channel_id: "channel-1234".to_string(),
            },
            IbcEndpoint {
                port_id: CONTRACT_PORT.to_string(),
                channel_id: my_channel.to_string(),
            },
            3,
            Timestamp::from_seconds(1665321069).into(),
        ))
    }

    fn mock_rcv_action_packet(
        action: OsmoPacket,
        channel: &str,
        amount: u128,
        denom: &str,
    ) -> IbcPacketReceiveMsg {
        let packet_data = mock_ics20_data(amount, denom, "", Some(action));
        let packet = mock_ibc_rcv_packet(channel, &packet_data);

        packet
    }

    fn assert_submsg_wasm<T: DeserializeOwned + std::cmp::PartialEq>(
        submsg: SubMsg,
        reply_id: u64,
        on: ReplyOn,
        contract: &String,
        msg_exp: T,
        mgs_funds: Vec<Coin>,
    ) {
        assert!(matches!(submsg, SubMsg {
            id,
            ref reply_on,
            msg: CosmosMsg::Wasm(WasmMsg::Execute {
                ref contract_addr,
                ref msg,
                funds,
            }),
            ..
        } if id == reply_id && reply_on.clone() == on && contract_addr.eq(contract) && funds.eq(&mgs_funds) && msg_exp.eq(&from_binary::<T>(msg).unwrap())));
    }

    #[test]
    fn send_receive_native() {
        let send_channel = "channel-9";
        let mut deps = setup(&["channel-1", "channel-7", send_channel]);

        let denom = "uatom";

        // prepare some mock packets
        let recv_packet = mock_receive_packet(send_channel, 876543210, denom, "local-rcpt");
        let recv_high_packet = mock_receive_packet(send_channel, 1876543210, denom, "local-rcpt");

        // cannot receive this denom yet
        let res = ibc_packet_receive(deps.as_mut(), mock_env(), recv_packet.clone()).unwrap();
        assert!(res.messages.is_empty());
        let ack: Ics20Ack = from_binary(&res.acknowledgement).unwrap();
        let no_funds = Ics20Ack::Error(ContractError::InsufficientFunds {}.to_string());
        assert_eq!(ack, no_funds);

        // we transfer some tokens
        let msg = ExecuteMsg::Transfer(TransferMsg {
            channel: send_channel.to_string(),
            remote_address: "my-remote-address".to_string(),
            timeout: None,
        });
        let info = mock_info("local-sender", &coins(987654321, denom));
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query channel state|_|
        let state = query_channel(deps.as_ref(), send_channel.to_string()).unwrap();
        assert_eq!(state.balances, vec![Amount::native(987654321, denom)]);
        assert_eq!(state.total_sent, vec![Amount::native(987654321, denom)]);

        // cannot receive more than we sent
        let res = ibc_packet_receive(deps.as_mut(), mock_env(), recv_high_packet).unwrap();
        assert!(res.messages.is_empty());
        let ack: Ics20Ack = from_binary(&res.acknowledgement).unwrap();
        assert_eq!(ack, no_funds);

        // we can receive less than we sent
        let res = ibc_packet_receive(deps.as_mut(), mock_env(), recv_packet).unwrap();
        assert_eq!(1, res.messages.len());
        assert_eq!(
            native_payment(876543210, denom, "local-rcpt"),
            res.messages[0]
        );
        let ack: Ics20Ack = from_binary(&res.acknowledgement).unwrap();
        assert!(matches!(ack, Ics20Ack::Result(_)));

        // only need to call reply block on error case

        // query channel state
        let state = query_channel(deps.as_ref(), send_channel.to_string()).unwrap();
        assert_eq!(state.balances, vec![Amount::native(111111111, denom)]);
        assert_eq!(state.total_sent, vec![Amount::native(987654321, denom)]);
    }

    #[test]
    fn receive_swap_action() {
        let send_channel = "channel-9";
        let mut deps = setup(&["channel-1", "channel-7", send_channel]);
        let denom = "uatom";
        let swap_denom = "uosmo";

        println!("hello");

        let swap = OsmoPacket::Swap(SwapPacket {
            routes: vec![SwapAmountInRoute {
                pool_id: 1u8.into(),
                token_out_denom: swap_denom.to_string(),
            }],
            token_out_min_amount: 1u8.into(),
        });

        let swap_packet_data = mock_ics20_data(876543210, denom, "", Some(swap));

        // prepare some mock packets
        let swap_packet = mock_ibc_rcv_packet(send_channel, &swap_packet_data);

        // we transfer some tokens
        let msg = ExecuteMsg::Transfer(TransferMsg {
            channel: send_channel.to_string(),
            remote_address: "my-remote-address".to_string(),
            timeout: None,
        });
        let info = mock_info("local-sender", &coins(987654321, denom));
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query channel state|_|
        let state = query_channel(deps.as_ref(), send_channel.to_string()).unwrap();
        assert_eq!(state.balances, vec![Amount::native(987654321, denom)]);
        assert_eq!(state.total_sent, vec![Amount::native(987654321, denom)]);

        // Swap action
        let res = ibc_packet_receive(deps.as_mut(), mock_env(), swap_packet).unwrap();
        assert_eq!(1, res.messages.len());
        check_gamm_submsg(res.messages[0].clone(), SWAP_ID, "swap").unwrap();

        let ack: Ics20Ack = from_binary(&res.acknowledgement).unwrap();
        assert!(matches!(ack, Ics20Ack::Result(_)));

        // Simulate swap reply
        let r = mock_swap_response();
        let reply_msg = mock_reply_msg(SWAP_ID, r.events, r.data);

        let res = reply(deps.as_mut(), mock_env(), reply_msg).unwrap();
        assert_eq!(0, res.messages.len());
        let gamm_ack: AmountResultAck = get_ack_result(&res.data.unwrap()).unwrap();
        let gamm_ack_exp = AmountResultAck {
            amount: Uint128::new(36601070u128),
            denom: swap_denom.to_string(),
        };
        assert_eq!(gamm_ack, gamm_ack_exp);

        // query channel state
        let state = query_channel(deps.as_ref(), send_channel.to_string()).unwrap();
        assert_eq!(
            state.balances,
            vec![
                Amount::native(111111111, denom),
                Amount::native(36601070, swap_denom)
            ]
        );
        assert_eq!(
            state.total_sent,
            vec![
                Amount::native(987654321, denom),
                Amount::native(36601070, swap_denom)
            ]
        );
    }
}
