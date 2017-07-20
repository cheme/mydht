extern crate tunnel;
use peer::Peer as MPeer;
use self::tunnel::Peer as TPeer;

impl<P : MPeer>  TPeer for P {
}
