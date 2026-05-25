O que aprendemos que é certo
O DAP + codelldb funciona. No run com stopOnEntry: true vimos:

Handshake completo: initialize → initialized → setBreakpoints → configurationDone ✓
Breakpoint resolvido: event/breakpoint "Resolved locations: 1" (evento [16]) ✓
Processo lançado, todos os módulos carregados ✓
Exception stop (loader INT3 do Windows) recebido corretamente ✓
O protocolo funciona. O problema está em outro lugar.

O problema real
Depois de continuar do loader INT3, o próximo recv retorna Content-Length: 0. Isso não é parsing error nosso — é o codelldb enviando uma mensagem vazia. Isso acontece antes de qualquer stopped reason=breakpoint.

Olha quem está injetando DLL no processo (eventos [24-27]):


bdhkm64.dll — Bitdefender Endpoint Security (Hooking Module)
atcuf64.dll — Bitdefender Endpoint Security (ATCUF)
Esses são módulos de monitoramento comportamental do Bitdefender que se injetam em todo processo que inicia. O Content-Length: 0 que o codelldb manda logo depois do continue é provável sinal de que o Bitdefender matou o processo antes de chegar na linha 20.

Evidência direta: o próprio Bitdefender já bloqueou um comando nosso durante essa sessão.

Verificação rápida
Antes de qualquer coisa, rode o executável direto — sem debugger:


.\target\debug\debug-target-example.exe
Se rodar e imprimir o array ordenado, o processo em si funciona. Se o Bitdefender matar, não vai imprimir nada.

Depois, adicione a pasta ao exclusion list do Bitdefender:

C:\workspace\rdc_app\target\debug\

E tente o script de novo. Se o processo sobreviver ao continue, veremos o stopped reason=breakpoint.

O stopOnEntry: false crasha mais cedo
Com false, o codelldb lida com o loader INT3 internamente (sem pausar), o processo sai correndo e o Bitdefender o mata ainda durante a carga dos módulos — por isso crasha no evento [15], antes mesmo de ver os outros DLLs carregarem.

Com true, o processo fica congelado no INT3 enquanto o Bitdefender injeta seus DLLs (eventos [24-27]). Daí quando continuamos, o Bitdefender já está dentro do processo e provavelmente manda matar.

Conclusão
O DAP + codelldb não é fajuto. Funciona — o VS Code usa exatamente isso. O bloqueio é ambiental: o Bitdefender Endpoint Security está impedindo o debug. O script não tem mais bugs de protocolo relevantes — o próximo passo é confirmar que o processo consegue rodar sob debugger sem ser morto pelo AV.