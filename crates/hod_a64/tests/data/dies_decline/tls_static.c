_Thread_local int file_tls = 7;
int bump(void) {
    static _Thread_local int counter = 5;
    counter++;
    return counter + file_tls;
}
int main(void) { return bump() == 13 ? 0 : 1; }
