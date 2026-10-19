@0x9c46aa8d73baf531;

enum Role { user @0; admin @1; }
struct Address { city @0 :Text; zip @1 :UInt32; }
struct MaybeText { union { none @0 :Void; some @1 :Text; } }
struct MaybeAddress { union { none @0 :Void; some @1 :Address; } }
struct TextEntry { key @0 :Text; value @1 :Text; }
struct Person {
  id @0 :UInt64;
  fullName @1 :Text;
  age @2 :UInt16;
  address @3 :Address;
  phones @4 :List(Text);
  scores @5 :List(Int64);
  coordinates @6 :List(Float64);
  flags @7 :List(Bool);
  avatar @8 :Data;
  nickname @9 :MaybeText;
  labels @10 :List(TextEntry);
  role @11 :Role;
  large @12 :Text;
  matrix @13 :List(List(UInt32));
  history @14 :List(Address);
  alternative @15 :MaybeAddress;
  token @16 :UInt64;
}
struct Event {
  union {
    ping @0 :Void;
    name @1 :Text;
    moved @2 :Address;
    login :group { user @3 :Text; count @4 :UInt32; }
  }
}
struct Number { value @0 :UInt64; }
struct Pair { item0 @0 :Text; item1 @1 :Int32; }
struct Empty {}
struct AsyncUser { name @0 :Text; }
struct ScalarText { date @0 :Text; amount @1 :Text; }
