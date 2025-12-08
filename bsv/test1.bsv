import FIFO::*;
typedef enum{
	Red=1,
	Blue=20,
	Green,
	Black=40 
} Colors_e deriving(Bits, Eq, FShow);
typedef struct{
	Colors_e rgb;
	Bit#(21) b;
} Foo_st deriving(Bits, Eq, FShow);
typedef struct {
	Foo_st f;
	Bit#(10)a;
	Foo_st t;
} Bar_st deriving(Bits, Eq, FShow);

interface Ifc_a;
	method Action in(Bit#(32) x, Bar_st b);
		method Foo_st out;
endinterface

(*synthesize*)
module mkA(Ifc_a);
	Reg#(Bar_st) rb <-mkRegA(unpack(0));
	method Action in(Bit#(32) x, Bar_st b);
		endmethod
		method Foo_st out;
			return unpack(0);
		endmethod

endmodule
module mkB(Ifc_a);
	Reg#(Bar_st) bar_ax <-mkRegA(unpack(0));
	Ifc_a inst_a <- mkA();
	FIFO#(Foo_st) ff <-mkFIFO();
	method Action in(Bit#(32) x, Bar_st b);
		endmethod
		method Foo_st out;
			return unpack(0);
		endmethod

endmodule
(*synthesize*)
module mkTop(Empty);
	Reg#(Foo_st) rb <-mkRegA(unpack(0));
	Ifc_a aa <-mkB();
	Ifc_a ab <-mkB();
	Ifc_a ac <-mkA();

endmodule
