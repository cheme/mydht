//! tunnel definition for this crate
use tunnel::full::{
  Full,
  GenTunnelTraits,
};


/// route provider
pub struct Rp<RP> {
  pub peers : Vec<RP>,

}

struct ReplyTraits<P : Peer>(PhantomData<P>);
struct TestTunnelTraits<P : Peer>(PhantomData<P>);
/// currently same conf as in tunnel crate tests
impl<P : Peer> GenTunnelTraits for ReplyTraits<P> {
  type P = P;
  type SSW = SWrite;
  type SSR = SRead;
  type TC = Nope;
  type LW = SizedWindows<TestSizedWindows>;
  type LR = SizedWindows<TestSizedWindows>;
  type RP = Nope;
  type RW = Nope;
  type REP = ReplyInfoProvider<
    Self::P,
    Self::SSW,
    Self::SSR,
    Self::SP,
  >;
  type SP = SProv;
  type EP = NoErrorProvider;
  type TNR = TunnelNope<P>;
  type EW = ErrorWriter;
}
/// currently same conf as in tunnel crate tests
impl<P : Peer> GenTunnelTraits for TestTunnelTraits<P> {
  type EW = ErrorWriter;
  type P = P;
  type SSW = SWrite;
  type SSR = SRead;
  type TC = CachedInfoManager<P>;
  type LW = SizedWindows<TestSizedWindows>;
  type LR = SizedWindows<TestSizedWindows>;
  type RP = Rp<P>;
  type TNR = Full<ReplyTraits<P>>;
//pub struct ReplyInfoProvider<E : ExtWrite + Clone, TNR : TunnelNoRep,SSW,SSR, SP : SymProvider<SSW,SSR>, RP : RouteProvider<TNR::P>> {
//impl<E : ExtWrite + Clone,P : Peer,TW : TunnelWriter, TNR : TunnelNoRep<P=P,TW=TW>,SSW,SSR,SP : SymProvider<SSW,SSR>,RP : RouteProvider<P>> ReplyProvider<P, ReplyInfo<E,P,TW>,SSW,SSR> for ReplyInfoProvider<E,TNR,SSW,SSR,SP,RP> {
//
//impl<E : ExtWrite, P : Peer, RI : RepInfo, EI : Info> TunnelWriter for FullW<RI,EI,P,E> {
//type TW = FullW<ReplyInfo<TT::LW,TT::P,TT::RW>, MultiErrorInfo<TT::LW,TT::RW>, TT::P, TT::LW>;
  type RW = FullW<MultipleReplyInfo<<Self::P as Peer>::Address>, MultipleErrorInfo,Self::P, Self::LW,Nope>;
  //type RW = TunnelWriterFull<FullW<MultipleReplyInfo<Self::LW,Self::P,Nope>, MultiErrorInfo<Self::LW,Nope>,Self::P, Self::LW>>;
  type REP = ReplyInfoProvider<
//    SizedWindows<TestSizedWindows>,
//    Full<ReplyTraits<P>>,
    Self::P,
    Self::SSW,
    Self::SSR,
    Self::SP,
  >;
  type SP = SProv;
  type EP = MulErrorProvider;
}
